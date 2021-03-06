/// A page, can be a blog post or a basic page
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::result::Result as StdResult;


use tera::{Tera, Context as TeraContext};
use serde::ser::{SerializeStruct, self};
use slug::slugify;

use errors::{Result, ResultExt};
use config::Config;
use utils::fs::{read_file, find_related_assets};
use utils::site::get_reading_analytics;
use utils::templates::render_template;
use front_matter::{PageFrontMatter, InsertAnchor, split_page_content};
use rendering::{Context, Header, markdown_to_html};

use file_info::FileInfo;


#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    /// All info about the actual file
    pub file: FileInfo,
    /// The front matter meta-data
    pub meta: PageFrontMatter,
    /// The actual content of the page, in markdown
    pub raw_content: String,
    /// All the non-md files we found next to the .md file
    pub assets: Vec<PathBuf>,
    /// The HTML rendered of the page
    pub content: String,
    /// The slug of that page.
    /// First tries to find the slug in the meta and defaults to filename otherwise
    pub slug: String,
    /// The URL path of the page
    pub path: String,
    /// The full URL for that page
    pub permalink: String,
    /// The summary for the article, defaults to None
    /// When <!-- more --> is found in the text, will take the content up to that part
    /// as summary
    pub summary: Option<String>,
    /// The previous page, by whatever sorting is used for the index/section
    pub previous: Option<Box<Page>>,
    /// The next page, by whatever sorting is used for the index/section
    pub next: Option<Box<Page>>,
    /// Toc made from the headers of the markdown file
    pub toc: Vec<Header>,
}


impl Page {
    pub fn new<P: AsRef<Path>>(file_path: P, meta: PageFrontMatter) -> Page {
        let file_path = file_path.as_ref();

        Page {
            file: FileInfo::new_page(file_path),
            meta: meta,
            raw_content: "".to_string(),
            assets: vec![],
            content: "".to_string(),
            slug: "".to_string(),
            path: "".to_string(),
            permalink: "".to_string(),
            summary: None,
            previous: None,
            next: None,
            toc: vec![],
        }
    }

    pub fn is_draft(&self) -> bool {
        self.meta.draft.unwrap_or(false)
    }

    /// Parse a page given the content of the .md file
    /// Files without front matter or with invalid front matter are considered
    /// erroneous
    pub fn parse(file_path: &Path, content: &str, config: &Config) -> Result<Page> {
        let (meta, content) = split_page_content(file_path, content)?;
        let mut page = Page::new(file_path, meta);
        page.raw_content = content;
        page.slug = {
            if let Some(ref slug) = page.meta.slug {
                slug.trim().to_string()
            } else {
                if page.file.name == "index" {
                    if let Some(parent) = page.file.path.parent() {
                        slugify(parent.file_name().unwrap().to_str().unwrap())
                    } else {
                        slugify(page.file.name.clone())
                    }
                } else {
                    slugify(page.file.name.clone())
                }
            }
        };

        if let Some(ref p) = page.meta.path {
            page.path = p.trim().trim_left_matches('/').to_string();

        } else {
            page.path = if page.file.components.is_empty() {
                page.slug.clone()
            } else {
                format!("{}/{}", page.file.components.join("/"), page.slug)
            };
        }
        if !page.path.ends_with('/') {
            page.path = format!("{}/", page.path);
        }

        page.permalink = config.make_permalink(&page.path);

        Ok(page)
    }

    /// Read and parse a .md file into a Page struct
    pub fn from_file<P: AsRef<Path>>(path: P, config: &Config) -> Result<Page> {
        let path = path.as_ref();
        let content = read_file(path)?;
        let mut page = Page::parse(path, &content, config)?;
        page.assets = vec![];

        if page.file.name == "index" {
            page.assets = find_related_assets(path.parent().unwrap());
        }

        Ok(page)

    }

    /// We need access to all pages url to render links relative to content
    /// so that can't happen at the same time as parsing
    pub fn render_markdown(&mut self, permalinks: &HashMap<String, String>, tera: &Tera, config: &Config, anchor_insert: InsertAnchor) -> Result<()> {
        let context = Context::new(
            tera,
            config.highlight_code.unwrap(),
            config.highlight_theme.clone().unwrap(),
            &self.permalink,
            permalinks,
            anchor_insert
        );
        let res = markdown_to_html(&self.raw_content, &context)?;
        self.content = res.0;
        self.toc = res.1;
        if self.raw_content.contains("<!-- more -->") {
            self.summary = Some({
                let summary = self.raw_content.splitn(2, "<!-- more -->").collect::<Vec<&str>>()[0];
                markdown_to_html(summary, &context)?.0
            })
        }

        Ok(())
    }

    /// Renders the page using the default layout, unless specified in front-matter
    pub fn render_html(&self, tera: &Tera, config: &Config) -> Result<String> {
        let tpl_name = match self.meta.template {
            Some(ref l) => l.to_string(),
            None => "page.html".to_string()
        };

        let mut context = TeraContext::new();
        context.add("config", config);
        context.add("page", self);
        context.add("current_url", &self.permalink);
        context.add("current_path", &self.path);

        render_template(&tpl_name, tera, &context, config.theme.clone())
            .chain_err(|| format!("Failed to render page '{}'", self.file.path.display()))
    }
}

impl Default for Page {
    fn default() -> Page {
        Page {
            file: FileInfo::default(),
            meta: PageFrontMatter::default(),
            raw_content: "".to_string(),
            assets: vec![],
            content: "".to_string(),
            slug: "".to_string(),
            path: "".to_string(),
            permalink: "".to_string(),
            summary: None,
            previous: None,
            next: None,
            toc: vec![],
        }
    }
}

impl ser::Serialize for Page {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error> where S: ser::Serializer {
        let mut state = serializer.serialize_struct("page", 16)?;
        state.serialize_field("content", &self.content)?;
        state.serialize_field("title", &self.meta.title)?;
        state.serialize_field("description", &self.meta.description)?;
        state.serialize_field("date", &self.meta.date)?;
        state.serialize_field("slug", &self.slug)?;
        state.serialize_field("path", &self.path)?;
        state.serialize_field("permalink", &self.permalink)?;
        state.serialize_field("summary", &self.summary)?;
        state.serialize_field("tags", &self.meta.tags)?;
        state.serialize_field("category", &self.meta.category)?;
        state.serialize_field("extra", &self.meta.extra)?;
        let (word_count, reading_time) = get_reading_analytics(&self.raw_content);
        state.serialize_field("word_count", &word_count)?;
        state.serialize_field("reading_time", &reading_time)?;
        state.serialize_field("previous", &self.previous)?;
        state.serialize_field("next", &self.next)?;
        state.serialize_field("toc", &self.toc)?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Write;
    use std::fs::{File, create_dir};
    use std::path::Path;

    use tera::Tera;
    use tempdir::TempDir;

    use config::Config;
    use super::Page;
    use front_matter::InsertAnchor;


    #[test]
    fn test_can_parse_a_valid_page() {
        let content = r#"
+++
title = "Hello"
description = "hey there"
slug = "hello-world"
+++
Hello world"#;
        let res = Page::parse(Path::new("post.md"), content, &Config::default());
        assert!(res.is_ok());
        let mut page = res.unwrap();
        page.render_markdown(&HashMap::default(), &Tera::default(), &Config::default(), InsertAnchor::None).unwrap();

        assert_eq!(page.meta.title.unwrap(), "Hello".to_string());
        assert_eq!(page.meta.slug.unwrap(), "hello-world".to_string());
        assert_eq!(page.raw_content, "Hello world".to_string());
        assert_eq!(page.content, "<p>Hello world</p>\n".to_string());
    }

    #[test]
    fn test_can_make_url_from_sections_and_slug() {
        let content = r#"
    +++
    slug = "hello-world"
    +++
    Hello world"#;
        let mut conf = Config::default();
        conf.base_url = "http://hello.com/".to_string();
        let res = Page::parse(Path::new("content/posts/intro/start.md"), content, &conf);
        assert!(res.is_ok());
        let page = res.unwrap();
        assert_eq!(page.path, "posts/intro/hello-world/");
        assert_eq!(page.permalink, "http://hello.com/posts/intro/hello-world/");
    }

    #[test]
    fn can_make_url_from_slug_only() {
        let content = r#"
    +++
    slug = "hello-world"
    +++
    Hello world"#;
        let config = Config::default();
        let res = Page::parse(Path::new("start.md"), content, &config);
        assert!(res.is_ok());
        let page = res.unwrap();
        assert_eq!(page.path, "hello-world/");
        assert_eq!(page.permalink, config.make_permalink("hello-world"));
    }

    #[test]
    fn can_make_url_from_path() {
        let content = r#"
    +++
    path = "hello-world"
    +++
    Hello world"#;
        let config = Config::default();
        let res = Page::parse(Path::new("content/posts/intro/start.md"), content, &config);
        assert!(res.is_ok());
        let page = res.unwrap();
        assert_eq!(page.path, "hello-world/");
        assert_eq!(page.permalink, config.make_permalink("hello-world"));
    }

    #[test]
    fn can_make_url_from_path_starting_slash() {
        let content = r#"
    +++
    path = "/hello-world"
    +++
    Hello world"#;
        let config = Config::default();
        let res = Page::parse(Path::new("content/posts/intro/start.md"), content, &config);
        assert!(res.is_ok());
        let page = res.unwrap();
        assert_eq!(page.path, "hello-world/");
        assert_eq!(page.permalink, config.make_permalink("hello-world"));
    }

    #[test]
    fn errors_on_invalid_front_matter_format() {
        // missing starting +++
        let content = r#"
    title = "Hello"
    description = "hey there"
    slug = "hello-world"
    +++
    Hello world"#;
        let res = Page::parse(Path::new("start.md"), content, &Config::default());
        assert!(res.is_err());
    }

    #[test]
    fn can_make_slug_from_non_slug_filename() {
        let config = Config::default();
        let res = Page::parse(Path::new(" file with space.md"), "+++\n+++", &config);
        assert!(res.is_ok());
        let page = res.unwrap();
        assert_eq!(page.slug, "file-with-space");
        assert_eq!(page.permalink, config.make_permalink(&page.slug));
    }

    #[test]
    fn can_specify_summary() {
        let config = Config::default();
        let content = r#"
+++
+++
Hello world
<!-- more -->"#.to_string();
        let res = Page::parse(Path::new("hello.md"), &content, &config);
        assert!(res.is_ok());
        let mut page = res.unwrap();
        page.render_markdown(&HashMap::default(), &Tera::default(), &config, InsertAnchor::None).unwrap();
        assert_eq!(page.summary, Some("<p>Hello world</p>\n".to_string()));
    }

    #[test]
    fn page_with_assets_gets_right_info() {
        let tmp_dir = TempDir::new("example").expect("create temp dir");
        let path = tmp_dir.path();
        create_dir(&path.join("content")).expect("create content temp dir");
        create_dir(&path.join("content").join("posts")).expect("create posts temp dir");
        let nested_path = path.join("content").join("posts").join("with-assets");
        create_dir(&nested_path).expect("create nested temp dir");
        let mut f = File::create(nested_path.join("index.md")).unwrap();
        f.write_all(b"+++\n+++\n").unwrap();
        File::create(nested_path.join("example.js")).unwrap();
        File::create(nested_path.join("graph.jpg")).unwrap();
        File::create(nested_path.join("fail.png")).unwrap();

        let res = Page::from_file(
            nested_path.join("index.md").as_path(),
            &Config::default()
        );
        assert!(res.is_ok());
        let page = res.unwrap();
        assert_eq!(page.file.parent, path.join("content").join("posts"));
        assert_eq!(page.slug, "with-assets");
        assert_eq!(page.assets.len(), 3);
        assert_eq!(page.permalink, "http://a-website.com/posts/with-assets/");
    }

    #[test]
    fn page_with_assets_and_slug_overrides_path() {
        let tmp_dir = TempDir::new("example").expect("create temp dir");
        let path = tmp_dir.path();
        create_dir(&path.join("content")).expect("create content temp dir");
        create_dir(&path.join("content").join("posts")).expect("create posts temp dir");
        let nested_path = path.join("content").join("posts").join("with-assets");
        create_dir(&nested_path).expect("create nested temp dir");
        let mut f = File::create(nested_path.join("index.md")).unwrap();
        f.write_all(b"+++\nslug=\"hey\"\n+++\n").unwrap();
        File::create(nested_path.join("example.js")).unwrap();
        File::create(nested_path.join("graph.jpg")).unwrap();
        File::create(nested_path.join("fail.png")).unwrap();

        let res = Page::from_file(
            nested_path.join("index.md").as_path(),
            &Config::default()
        );
        assert!(res.is_ok());
        let page = res.unwrap();
        assert_eq!(page.file.parent, path.join("content").join("posts"));
        assert_eq!(page.slug, "hey");
        assert_eq!(page.assets.len(), 3);
        assert_eq!(page.permalink, "http://a-website.com/posts/hey/");
    }
}
