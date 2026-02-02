use anyhow::{Context, anyhow};
use clap::Parser;

use chrono::{DateTime, Local};
use rss::{Channel, ChannelBuilder, Item, validation::Validate};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, BufReader, ErrorKind, Read},
    path::{Path, PathBuf},
};

/// This app generates or updates RSS feeds from a directory with html files.
/// Can be useful for simple static blogs.
#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Path to the directory with html files
    #[arg(long)]
    dir: PathBuf,
    /// Base URL of the site to be used for links in the resulting feed
    #[arg(long)]
    base_url: String,
    /// Title of the feed
    #[arg(long)]
    title: String,
    /// The HTML tag used to parse titles of RSS items (can be a hidden tag).
    /// Only the first occurence of the tag is considered. Default value is
    /// "title" so if the document contains `<title>Article Title</title>` or
    /// `<title hidden>Article Title</title>``, the resulting item will have
    /// "Article Title" as a title
    #[arg(long, default_value = "title")]
    article_title_tag: String,
    /// The HTML tag used to parse dates of RSS items (can be a hidden tag).
    /// Only the first occurence of the tag is considered. Dates must be in
    /// RFC 2822. existing dates in the feed take precedence over the dates
    /// set through this tag . When no dates are detected, the dates are set
    /// to the current date and time in the feed. Examples: `<date>Wed, 28 Jan
    /// 2026 20:13:42 GMT</date>` or `<date hidden>Wed, 28 Jan 2026 20:13:42
    /// GMT</date>`
    #[arg(long)]
    article_date_tag: Option<String>,
    /// The filename to read from/write to
    #[arg(long, default_value = "feed.xml")]
    feed_filename: String,
}

pub(crate) struct App {
    pub(crate) cli: Cli,
    links_to_items: HashMap<String, Item>,
}

impl App {
    pub fn new(cli: Cli) -> anyhow::Result<Self> {
        let file_result = File::open(&cli.feed_filename);
        if let Err(ref e) = file_result
            && e.kind() == ErrorKind::NotFound
        {
            // this is a new feed
            return Ok(Self {
                cli,
                links_to_items: HashMap::new(),
            });
        };

        let file =
            file_result.with_context(|| format!("can't open feed file {}", cli.feed_filename))?;

        let channel = Channel::read_from(BufReader::new(file))
            .with_context(|| format!("can't read feed file {}", cli.feed_filename))?;

        let mut links_to_items: HashMap<String, Item> = HashMap::new();
        for i in channel.items() {
            if let Some(link) = i.link() {
                links_to_items.insert(link.to_string(), i.clone());
            } else {
                return Err(anyhow!(
                    "item with title {:?} does not have a link",
                    i.title()
                ));
            }
        }

        Ok(Self {
            cli,
            links_to_items,
        })
    }

    fn get_link(&self, file_path: &PathBuf) -> String {
        Path::new(&self.cli.base_url)
            .join(file_path)
            .to_string_lossy()
            .to_string()
    }

    pub(crate) fn run(&self) -> anyhow::Result<()> {
        let mut channel = ChannelBuilder::default()
            .title(&self.cli.title)
            .link(&self.cli.base_url)
            .description("RSS feed of ".to_owned() + &self.cli.title)
            .build();

        let dir_entries = fs::read_dir(&self.cli.dir).with_context(|| {
            format!(
                "can't read directory with html files: {}",
                self.cli.dir.display(),
            )
        })?;

        let mut html_files = Vec::new();
        for res in dir_entries {
            let entry = res.with_context(|| {
                format!(
                    "can't read dir entry from directory {}",
                    self.cli.dir.display()
                )
            })?;

            if !entry.path().is_file() {
                continue;
            }

            if let Some(ext) = entry.path().extension()
                && ext == "html"
            {
                html_files.push(entry.path());
            }
        }

        for file_path in html_files {
            log::info!("processing file {}", file_path.display());
            let mut contents = String::new();
            File::open(&file_path)
                .and_then(|mut file| file.read_to_string(&mut contents))
                .with_context(|| format!("can't open html file {}", file_path.display()))?;

            let item = self.html_to_item(contents, file_path)?;
            channel.items.push(item)
        }

        channel
            .validate()
            .context("can't validate the newly generated channel")?;

        let file = fs::File::create(&self.cli.feed_filename)
            .with_context(|| format!("can't create feed file {}", self.cli.feed_filename))?;

        channel
            .write_to(io::BufWriter::new(file))
            .map(|_| ())
            .with_context(|| format!("can't write to feed file {}", self.cli.feed_filename))
    }

    fn html_to_item(&self, html: String, file_path: PathBuf) -> anyhow::Result<rss::Item> {
        let mut item: Item = Item::default();

        let title = parse_tag(&html, &self.cli.article_title_tag).with_context(|| {
            format!(
                "html tag {} not found in file {}",
                file_path.display(),
                self.cli.article_title_tag
            )
        })?;

        log::debug!("title: {:?}", title);
        item.set_title(Some(title.to_string()));

        let link = self.get_link(&file_path);

        let existing_rss_item_date = self
            .links_to_items
            .get(&link)
            .and_then(|item| item.pub_date());

        let mut pub_date = None;
        if let Some(date) = existing_rss_item_date {
            pub_date = Some(
                DateTime::parse_from_rfc2822(date)
                    .with_context(|| {
                        format!(
                            "can't parse publication date for existing rss item `{}`; attempted to parse `{}` from file `{}`",
                            link, date, self.cli.feed_filename
                        )
                    })?
                    .to_rfc2822(),
            );
        } else if let Some(date_tag) = &self.cli.article_date_tag {
            if let Some(date) = parse_tag(&html, &date_tag) {
                pub_date = Some(
                    DateTime::parse_from_rfc2822(date)
                        .with_context(|| {
                            format!(
                                "can't parse publication date from html tag `{}`; attempted to parse `{}` from file `{}`",
                                date_tag, date, self.cli.feed_filename
                            )
                        })?
                        .to_rfc2822(),
                )
            }
        } else {
            pub_date = Some(Local::now().to_rfc2822());
        }

        item.set_pub_date(pub_date);

        item.set_link(Some(link));
        item.set_description(Some(html));
        Ok(item)
    }
}

fn parse_tag<'a, 'b>(buf: &'a str, tag: &'b str) -> Option<&'a str> {
    let opening_tag = &("<".to_owned() + tag + ">");
    let hidden_opening_tag = &("<".to_owned() + tag + " hidden>");
    let closing_tag = &("</".to_owned() + tag + ">");
    buf.split_once(opening_tag)
        .or_else(|| buf.split_once(hidden_opening_tag))
        .and_then(|(_, suffix)| suffix.split_once(closing_tag))
        .and_then(
            |(suffix, _prefix)| {
                if suffix.len() > 0 { Some(suffix) } else { None }
            },
        )
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::App;
    use crate::app::parse_tag;

    #[test]
    fn test_parse_tag() {
        assert_eq!(
            parse_tag("<html><title>test title</title></html>", "title"),
            Some("test title")
        );
        assert_eq!(
            parse_tag("<html><title hidden>test title</title></html>", "title"),
            Some("test title")
        );
        assert_eq!(
            parse_tag(
                "<html><title>test title</title><title>not test title</title></html>",
                "title"
            ),
            Some("test title")
        );
        assert_eq!(
            parse_tag("<html><title>test title</title></html>", "no_such_tag"),
            None,
        );
        assert_eq!(parse_tag("", "no_tags_at_all"), None,);
    }

    #[test]
    fn test_get_link() {
        let app = get_empty_app();
        assert_eq!(
            app.get_link(&PathBuf::from("my_file.html")),
            "http://localhost/my_file.html"
        )
    }

    fn get_empty_app() -> App {
        App {
            cli: crate::app::Cli {
                dir: PathBuf::from("test_dir"),
                base_url: "http://localhost/".to_string(),
                title: "Test Feed".to_string(),
                article_title_tag: "title".to_string(),
                article_date_tag: Some("date".to_string()),
                feed_filename: "feed.xml".to_string(),
            },
            links_to_items: HashMap::new(),
        }
    }
}
