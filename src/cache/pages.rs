
use rocket_contrib::Template;
use rocket::{Request, Data, Outcome, Response};
use rocket::response::{NamedFile, Redirect, Flash, Responder, Content};
use rocket::response::content::Html;
use rocket::data::FromData;
use rocket::request::{FlashMessage, Form, FromForm, FormItems, FromRequest};
use rocket::http::{Cookie, Cookies, MediaType, ContentType, Status};
use rocket::State;

use std::fmt::Display;
use std::{env, str, thread};
use std::fs::{self, File, DirEntry};
use std::io::prelude::*;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::{self, Instant, Duration};
use std::prelude::*;
use std::ffi::OsStr;
use std::collections::HashMap;
use std::sync::{Mutex, Arc, RwLock};
use std::sync::atomic::AtomicUsize;
use std::borrow::Cow;

use rss::{Channel, ChannelBuilder, Guid, GuidBuilder, Item, ItemBuilder, Category, CategoryBuilder, TextInput, TextInputBuilder, extension};
use chrono::{DateTime, TimeZone, NaiveDateTime, Utc};
use urlencoding::encode;

use evmap::*;
use comrak::{markdown_to_html, ComrakOptions};

use super::super::*;
use super::*;
use ::blog::*;
use ::data::*;
use ::content::*;
use ::templates::*;
use ::xpress::*;
use ::ral_user::*;
use ::ral_administrator::*;
use ::collate::*;

/*

    text        all_tags
    multi*      /tag/<tag>
                    /tag?<tag>
        
    article     /article?<aid>
                    /article/<aid>
                    /article/<aid>/<title>
                /article (hbs_article_not_found)
    text        /rss.xml
    multi*      /author/<authorid>
                    /author/<authorid>/<authorname>
    text        /about
        
        
    /pageviews
    /pagestats
    /pagestats/<show_errors>
    /manage/<sortstr>/<orderstr>
    /manage
    
*/


pub mod info {
    use super::*;
    pub fn info(title: Option<String>,
                page: String,
                admin: Option<AdministratorCookie>,
                user: Option<UserCookie>,
                gen: Option<GenTimer>,
                uhits: Option<UniqueHits>,
                encoding: Option<AcceptCompression>,
                msg: Option<String>,
                javascript: Option<String>,
               ) -> TemplateInfo
    {
        let js = if let Some(j) = javascript { j } else { "".to_string() };
        let (pages, admin_pages) = create_menu(&page, &admin, &user);
        let info = TemplateInfo::new(title, admin, user, js, gen.map(|g| g.0), page, pages, admin_pages, msg);
        info
    }
}


pub mod articles {
    use super::*;
    pub fn context<T: Collate>(conn: &DbConn,
                   pagination: Page<T>,
                   article_lock: &ArticleCacheLock,
                   admin: Option<AdministratorCookie>, 
                   user: Option<UserCookie>, 
                   gen: Option<GenTimer>, 
                   uhits: Option<UniqueHits>, 
                   encoding: Option<AcceptCompression>, 
                   msg: Option<String>,
                   javascript: Option<String>,
                   info_opt: Option<String>
                  ) -> Result<CtxBody<TemplateArticlesPages>, CtxBody<TemplateGeneral>>
    {
        let javascript: Option<String> = None;
        // macro_rules! ctx_info {
        //     ( $title:expr, $page:expr ) => {
        //         info::info(if $title == "" { None } else { Some($title.to_owned()) }, $page.to_owned(), admin, user, gen, uhits, encoding, javascript, msg)
        //     }
        // }
        // let i = ctx_info!("Article", "/");
        
        if let Some((articles, total_items)) = article_lock.paginated_articles(&pagination) {
            let i = info::info(None, "/".to_owned(), admin, user, gen, uhits, encoding, javascript, msg);
            Ok(CtxBody( TemplateArticlesPages::new(articles, pagination.clone(), total_items, info_opt, i) ))
        } else if let Some((articles, total_items)) = cache::pages::articles::fallback(conn, &pagination) {
            let i = info::info(None, "/".to_owned(), admin, user, gen, uhits, encoding, javascript, msg);
            if !PRODUCTION {
                println!("Serving all aticles from fallacbk instead of cache");
            }
            Ok(CtxBody( TemplateArticlesPages::new(articles, pagination, total_items, info_opt, i) ))
        } else {
            let i = info::info(Some(format!("No articles found")), "/".to_owned(), admin, user, gen, uhits, encoding, msg, javascript);
            Err(CtxBody( TemplateGeneral::new("No articles found.".to_owned(), i) ))
        }
    }
    pub fn fallback<T: Collate>(conn: &DbConn, pagination: &Page<T>) -> Option<(Vec<Article>, u32)> {
        unimplemented!()
    }
    #[inline]
    pub fn serve<T: Collate>(article_lock: &ArticleCacheLock, 
                 pagination: Page<T>,
                 conn: &DbConn, 
                 admin: Option<AdministratorCookie>, 
                 user: Option<UserCookie>, 
                 start: GenTimer, 
                 uhits: UniqueHits,
                 encoding: AcceptCompression,
                 msg: Option<String>,
                 info_opt: Option<String>
                ) -> Express 
    {
        let ctx = cache::pages::articles::context(conn,
                                                    pagination,
                                                    &*article_lock,
                                                    admin, 
                                                    user, 
                                                    Some(start), 
                                                    Some(uhits), 
                                                    Some(encoding),
                                                    None,
                                                    None,
                                                    info_opt,
                                                   );
        
        let express: Express = cache::template(ctx);
        express
    }
}

/// The article route module allows routes to serve up pages with
/// a single article as the content.
/// The article route module does not need a function to generate
/// the page, it only needs a serve function.
pub mod article {
    use super::*;
    pub fn context(aid: u32,
                   body: Option<Article>,
                   conn: &DbConn,
                   admin: Option<AdministratorCookie>, 
                   user: Option<UserCookie>, 
                   gen: Option<GenTimer>, 
                   uhits: Option<UniqueHits>, 
                   encoding: Option<AcceptCompression>, 
                   msg: Option<String>,
                   javascript: Option<String>
                  ) -> Result<CtxBody<TemplateArticle>, CtxBody<TemplateGeneral>>
    {
        // macro_rules! ctx_info {
        //     ( $title:expr, $page:expr ) => {
        //         info::info(if $title == "" { None } else { Some($title.to_owned()) }, $page.to_owned(), admin, user, gen, uhits, encoding, javascript, msg)
        //     }
        // }
        // let i = ctx_info!("Article", "/");
        
        if let Some(article) = body {
            let i = info::info(Some(article.title.clone()), "/article".to_owned(), admin, user, gen, uhits, encoding, msg, javascript);
            Ok(CtxBody( TemplateArticle::new(article, i) ))
        } else if let Some(article) = cache::pages::article::fallback(aid, conn) {
            let i = info::info(Some(article.title.clone()), "/article".to_owned(), admin, user, gen, uhits, encoding, msg, javascript);
            if !PRODUCTION {
                println!("Article {} served from fallacbk instead of cache", aid);
            }
            Ok(CtxBody( TemplateArticle::new(article, i) ))
        } else {
            let i = info::info(Some(format!("Article {} not found", aid)), "/article".to_owned(), admin, user, gen, uhits, encoding, msg, javascript);
            Err(CtxBody( TemplateGeneral::new("The article could not be found.".to_owned(), i) ))
        }
    }
    pub fn fallback(aid: u32, conn: &DbConn) -> Option<Article> {
        let id = ArticleId { aid };
        id.retrieve()
    }
    #[inline]
    pub fn serve(aid: u32, 
                 article_state: State<ArticleCacheLock>, 
                 conn: &DbConn, 
                 admin: Option<AdministratorCookie>, 
                 user: Option<UserCookie>, 
                 start: GenTimer, 
                 uhits: UniqueHits,
                 encoding: AcceptCompression,
                 msg: Option<String>
                ) -> Express 
    {
        let article_rst = article_state.retrieve_article(aid);
        
        let javascript = Some("enable_toc(true);".to_owned());
        
        let ctx: Result<CtxBody<TemplateArticle>, CtxBody<TemplateGeneral>>
             = cache::pages::article::context(aid,
                                              article_rst, 
                                              conn,
                                              admin, 
                                              user, 
                                              Some(start), 
                                              Some(uhits), 
                                              Some(encoding),
                                              None,
                                              javascript
                                             );
        cache::template(ctx)
    }
}

pub mod tag {
    use super::*;
    pub fn context<T: Collate>(tag: &str,
                   conn: &DbConn,
                   pagination: &Page<T>,
                   article_cache: &ArticleCacheLock,
                   multi_aids: &TagAidsLock,
                   admin: Option<AdministratorCookie>, 
                   user: Option<UserCookie>, 
                   uhits: Option<UniqueHits>, 
                   gen: Option<GenTimer>, 
                   encoding: Option<AcceptCompression>,
                   msg: Option<String>,
                   javascript: Option<String>
                  ) -> Result<CtxBody<TemplateArticlesPages>, CtxBody<TemplateGeneral>>
    {
        if CACHE_ENABLED {
            if let Some((articles, total_items)) = multi_aids.tag_articles(article_cache, tag, &pagination) {
                let javascript: Option<String> = None;
                let info_opt: Option<String> = None;
                let i = info::info( Some(format!("Showing articles with tag '{}'", &tag)), "/tag".to_owned(), admin, user, gen, uhits, encoding, msg, javascript );
                Ok(CtxBody( TemplateArticlesPages::new(articles, pagination.clone(), total_items, info_opt, i) ))
            } else {
                let i = info::info( Some(format!("No articles to display for tag '{}'", &tag)), "/tag".to_owned(), admin, user, gen, uhits, encoding, msg, javascript );
                Err(CtxBody( TemplateGeneral::new(format!("No artiles displayed for tag {}", tag), i) ))
            }
            
        } else if CACHE_FALLBACK {
            if let Some((articles, total_items)) = cache::pages::tag::fallback(tag, &pagination, conn) {
                let javascript: Option<String> = None;
                let info_opt: Option<String> = None;
                let i = info::info( Some(format!("Showing articles with tag '{}'", &tag)), "/tag".to_owned(), admin, user, gen, uhits, encoding, msg, javascript );
                Ok(CtxBody( TemplateArticlesPages::new(articles, pagination.clone(), total_items, info_opt, i) ))
            } else {
                let i = info::info( Some(format!("No articles to display for tag '{}'", &tag)), "/tag".to_owned(), admin, user, gen, uhits, encoding, msg, javascript );
                Err(CtxBody( TemplateGeneral::new(format!("No artiles displayed for tag {}", tag), i) ))
            }
        } else {
            println!("SUPER ERROR: Cache disabled and cache fallback disabled");
            let i = info::info( Some("Error".to_owned()), "/tag".to_owned(), admin, user, gen, uhits, encoding, msg, javascript );
            Err(CtxBody( TemplateGeneral::new("Error retrieving articles.".to_owned(), i) ))
        }
    }
    #[inline]
    pub fn serve<T: Collate>(tag: &str, 
                 pagination: &Page<T>,
                 multi_aids: &TagAidsLock, 
                 article_state: &ArticleCacheLock, 
                 conn: &DbConn, 
                 admin: Option<AdministratorCookie>, 
                 user: Option<UserCookie>, 
                 uhits: Option<UniqueHits>, 
                 gen: Option<GenTimer>, 
                 encoding: Option<AcceptCompression>,
                 msg: Option<String>,
                ) -> Express {
        use ::sanitize::sanitize_tag;
        let t = sanitize_tag(tag);
        let javascript: Option<String> = None;
        cache::template( cache::pages::tag::context(&t, conn, &pagination, article_state, multi_aids, admin, user, uhits, gen, encoding, msg, javascript) )
    }
    // This function is used to fill the multi article cache.  
    // This is used to cache what articles correspond with each tag
    pub fn load_tag_aids(conn: &DbConn, tag: &str) -> Option<Vec<u32>> {
        // look up all ArticleId's for the given tag
        let qrystr = format!("SELECT aid FROM articles WHERE '{}' = ANY(tag) ORDER BY modified DESC", tag.to_lowercase());
        let result = conn.query(&qrystr, &[]);
        if let Ok(rst) = result {
            let aids: Vec<u32> = rst.iter().map(|row| row.get(0)).collect();
            if aids.len() != 0 {
                Some(aids)
            } else {
                println!("ERROR LOADING TAG {} - no articles found\n'{}'", tag, &qrystr);
                None
            }
        } else if let Err(err) = result {
            println!("ERROR LOADING TAG: {} - {}\n'{}'", tag, err, &qrystr);
            None
        } else {
            println!("ERROR LOADING TAG: {}\n'{}'", tag, &qrystr);
            None
        }
    }
    // The fallback() should return the current page of articles and the total number of articles
    pub fn fallback<T: Collate>(tag: &str, pagination: &Page<T>, conn: &DbConn) -> Option<(Vec<Article>, u32)> {
        // conn.articles(&format!("SELECT a.aid, a.title, a.posted, a.body, a.tag, a.description, u.userid, u.display, u.username, a.image, a.markdown, a.modified  FROM articles a JOIN users u ON (a.author = u.userid) WHERE '{}' = ANY(tag)", tag))
        // Need to use collate's methods to help generate the SQL
        // use ArticleId.retrieve_with_conn(conn)
        unimplemented!()
    }
}

pub mod tags {
    use super::*;
    pub fn context(conn: &DbConn,
                   multi_aids: &TagAidsLock,
                   admin: Option<AdministratorCookie>, 
                   user: Option<UserCookie>, 
                   uhits: Option<UniqueHits>, 
                   gen: Option<GenTimer>, 
                   encoding: Option<AcceptCompression>,
                   msg: Option<String>,
                   javascript: Option<String>
                  ) -> Result<CtxBody<TemplateTags>, CtxBody<TemplateGeneral>> 
    {
        if let Some(all_tags) = multi_aids.retrieve_tags() {
            let i = info::info( Some("Tag Cloud".to_owned()), "/all_tags".to_owned(), admin, user, gen, uhits, encoding, msg, javascript );
            Ok(CtxBody( TemplateTags::new(all_tags.clone(), i) ))
        } else {
            let i = info::info( Some("Error".to_owned()), "/all_tags".to_owned(), admin, user, gen, uhits, encoding, msg, javascript );
            Err(CtxBody( TemplateGeneral::new("Error retrieving tags".to_owned(), i) ))
        }
    }
    #[inline]
    pub fn serve(conn: &DbConn, 
                 multi_aids: &TagAidsLock, 
                 admin: Option<AdministratorCookie>, 
                 user: Option<UserCookie>, 
                 uhits: Option<UniqueHits>, 
                 gen: Option<GenTimer>, 
                 encoding: Option<AcceptCompression>,
                 msg: Option<String>,
                ) -> Express 
    {
        let javascript: Option<String> = None;
        cache::template( cache::pages::tags::context(conn, multi_aids, admin, user, uhits, gen, encoding, msg, javascript) )
    }
}

pub mod author {
    use super::*;
    pub fn context<T: Collate>(author: u32,
                   pagination: &Page<T>,
                   conn: &DbConn,
                   multi_aids: &TagAidsLock,
                   article_lock: &ArticleCacheLock,
                   admin: Option<AdministratorCookie>, 
                   user: Option<UserCookie>, 
                   uhits: Option<UniqueHits>, 
                   gen: Option<GenTimer>, 
                   encoding: Option<AcceptCompression>,
                   msg: Option<String>,
                   javascript: Option<String>
    ) -> Result<CtxBody<TemplateArticlesPages>, CtxBody<TemplateGeneral>> {
        if let Some((articles, total_items)) = multi_aids.author_articles(article_lock, author, &pagination) {
            let javascript: Option<String> = None;
            let info_opt: Option<String> = None;
            let i = info::info( Some("Showing articles by author".to_owned()), "/author".to_owned(), admin, user, gen, uhits, encoding, msg, javascript );
                Ok(CtxBody( TemplateArticlesPages::new(articles, pagination.clone(), total_items, info_opt, i) ))
        } else {
            let i = info::info( Some( "No articles to display".to_owned() ), "/author".to_owned(), admin, user, gen, uhits, encoding, msg, javascript );
            Err(CtxBody( TemplateGeneral::new("No articles found for specified author.".to_owned(), i) ))
        }
    }
    #[inline]
    pub fn serve<T: Collate>(author: u32,
                 pagination: &Page<T>,
                 conn: &DbConn, 
                 multi_aids: &TagAidsLock, 
                 article_lock: &ArticleCacheLock,
                 admin: Option<AdministratorCookie>, 
                 user: Option<UserCookie>, 
                 uhits: Option<UniqueHits>, 
                 gen: Option<GenTimer>, 
                 encoding: Option<AcceptCompression>,
                 msg: Option<String>,
    ) -> Express {
        let javascript: Option<String> = None;
        cache::template( cache::pages::author::context(author, &pagination, conn, multi_aids, article_lock, admin, user, uhits, gen, encoding, msg, javascript) )
    }
    
    // Find all authors' user ids
    pub fn load_author_articles(conn: &DbConn, userid: u32) -> Option<Vec<u32>> {
        let qry = conn.query(&format!("SELECT aid FROM articles WHERE author = {} ORDER BY modified DESC", userid), &[]);
        if let Ok(result) = qry {
            let aids: Vec<u32> = result.iter().map(|row| row.get(0)).collect();
            Some(aids)
        } else {
            println!("Failed to load multi article cache for author {}", userid);
            None
        }
    }
    pub fn load_authors(conn: &DbConn) -> Vec<u32> {
        let qry = conn.query("SELECT userid FROM users", &[]);
        if let Ok(result) = qry {
            let users: Vec<u32> = result.iter().map(|row| row.get(0)).collect();
            users
        } else {
            println!("Failed to load multi article cache for authors.  Could not find any users.");
            Vec::new()
        }
        
    }
}

pub mod rss {
    use super::*;
    #[inline]
    pub fn serve(conn: &DbConn, 
                 text_lock: &TextCacheLock,
                 admin: Option<AdministratorCookie>, 
                 user: Option<UserCookie>, 
                 uhits: Option<UniqueHits>, 
                 gen: Option<GenTimer>, 
                 encoding: Option<AcceptCompression>,
                 msg: Option<String>,
                ) -> Express
    {
        // let javascript: Option<String> = None;
        let content = text_lock.retrieve_text("rss").unwrap_or("Could not load RSS feed.".to_owned());
        let express: Express = content.into();
        express.set_content_type(ContentType::XML)
    }
    pub fn serve_filter(key: String,
                        // tag: Option<String>,
                        // author: Option<u32>,
                        text_lock: &TextCacheLock,
                        // multi_aids: &TagAidsLock,
                        // article_lock: &ArticleCacheLock,
                        // admin: Option<AdministratorCookie>, 
                        // user: Option<UserCookie>, 
                        uhits: Option<UniqueHits>, 
                        gen: Option<GenTimer>, 
                        encoding: Option<AcceptCompression>,
                        // msg: Option<String>,
                       ) -> Express 
    {
        if let Some(feed) = text_lock.retrieve_text(&key) {
            // let mut output: Express = "No rss feed found for specified filter(s).".to_owned().into()
            let mut output: Express = feed.into();
            output.set_content_type(ContentType::XML)
        } else {
            println!("Error: requested filtered rss feed, no matching feed found.\nkey: {}", &key);
            let mut output: Express = "No rss feed found for specified filter(s).".to_owned().into();
            output
        }
    }
    
    pub fn load_filtered_rss(conn: &DbConn, article_cache: &ArticleCacheLock, multi_aids: &TagAidsLock) -> Option<Vec<(String, String)>> {
        let authors = cache::pages::author::load_authors(conn);
        let tags: Vec<TagCount> = if let Some(t) = multi_aids.retrieve_tags() {
            t
        } else {
            Vec::new()
        };
        
        // println!("Loading rss feed filters.\n    Authors: {:#?}\nTags: {:#?}", &authors, &tags);
        
        let mut feeds: Vec<(String, String)> = Vec::new();
        
        for author in &authors {
            let key = format!("rss-author/{}", author).to_lowercase();
            if let Some(feed) = filter_rss(article_cache, multi_aids, None, Some(*author), true) {
                feeds.push((key, feed));
            } else {
                println!("Error loading rss author filter feed: {}", &key);
            }
        }
        
        for tag in &tags {
            let key = format!("rss-tag/{}", tag.tag).to_lowercase();
            if let Some(feed) = filter_rss(article_cache, multi_aids, Some(&tag.tag), None, true) {
                feeds.push((key, feed));
            } else {
                println!("Error loading rss tag filter feed {}", &key);
            }
        }
        
        let output = feeds;
            
        if output.len() != 0 {
            Some(output)
        } else {
            None
        }
        // None
    }
    
    // pub fn filter_rss(conn: &DbConn, tag: Option<&str>, author: Option<u32>) -> Option<String>
    pub fn filter_rss(article_cache: &ArticleCacheLock, multi_aids: &TagAidsLock, tag: Option<&String>, author: Option<u32>, short_description: bool) -> Option<String>
    {
        if let Some(all_articles) = article_cache.all_articles() {
            let custom_title: Option<String>;
            let custom_link: Option<String>;
            
            let mut articles: Vec<Article> = Vec::with_capacity(article_cache.num_articles() as usize);
            
            // println!("Iterating through {} articles and matching against rss feed filter", all_articles.len());
            
            if let Some(ref tag) = tag {
                if let &Some(ref author) = &author {
                    // both tag and author
                    custom_title = Some(format!("Articles in #{} by userid {}", tag, author));
                    // The following link WILL NOT WORK!
                    // custom_link = format!("rss-tag-author/{}/{}", tag, author);
                    custom_link = Some(String::from("rss.xml"));
                    for article in all_articles {
                        if article.userid == *author && article.tags.contains(&tag.to_lowercase()) {
                            // println!("Adding article to filtered feed");
                            articles.push(
                                if short_description == false {
                                    article.clone()
                                } else {
                                    article.short_clone()
                                }
                            )
                        }
                    }
                } else {
                    // just tag
                    // println!("Looking for tag {} in articles", &tag);
                    custom_title = Some(format!("#{} articles", tag));
                    custom_link = Some(format!("rss-tag/{}", tag));
                    for article in all_articles {
                        if article.tags.contains(&tag.to_lowercase()) {
                            // println!("Adding article to filtered feed");
                            articles.push(
                                if short_description == false {
                                    article.clone()
                                } else {
                                    article.short_clone()
                                }
                            )
                        }
                    }
                }
            } else if let &Some(ref author) = &author {
                // just author
                custom_title = Some(format!("Userid {}", author));
                custom_link = Some(format!("rss-userid/{}", author));
                for article in all_articles {
                    if article.userid == *author {
                        // println!("Adding article to filtered feed");
                        articles.push(
                            if short_description == false {
                                article.clone()
                            } else {
                                article.short_clone()
                            }
                        )
                    }
                }
            } else {
                custom_title = None;
                custom_link = None;
                // all
                for article in all_articles {
                    // println!("Adding article to filtered feed");
                    articles.push(
                        if short_description == false {
                            article.clone()
                        } else {
                            article.short_clone()
                        }
                    )
                }
            }
            
            if articles.len() == 0 {
                println!("Could not create filtered rss feed: no filtered articles collected");
                return None;
            }
            
            let output = create_rss_feed( articles, custom_title.as_ref(), custom_link.as_ref() );
            
            {
                let out = &output;
                
                if out == "Could not create text input item in RSS channel."
                    || out == "Could not create RSS channel."
                    || out == "Could not create RSS feed." 
                {
                    println!("Could not create filtered rss feed: rss output error: '{}'", &out);
                    return None;
                }
            }
            Some(output)
            
        } else {
            println!("Could not create filtered rss feed: error returning all articles");
            None
        }
        
    }
    
    pub fn load_rss(conn: &DbConn) -> String {
        let result = conn.articles("");
        if let Some(articles) = result {
            rss_output(articles)
        } else {
            let output = String::from("Could not create RSS feed.");
            output
        }
    }
}

// Maybe at some point make the custom RSS feeds use the rss::CategoryBuilder
//     This would make each tag an explicit category instead of a separate RSS feed
//     but would also require making the rss into one large feed
fn create_rss_feed(articles: Vec<Article>, custom_title: Option<&String>, custom_link: Option<&String>) -> String {
    let mut article_items: Vec<Item> = Vec::new();
    for article in &articles {
        let mut link = String::with_capacity(BLOG_URL.len()+20);
        link.push_str(BLOG_URL);
        link.push_str("article/");
        link.push_str(&article.aid.to_string());
        link.push_str("/");
        link.push_str( &encode(&article.title) );
        
        let desc: &str = if &article.description != "" {
            &article.description
        } else {
            if article.body.len() > DESC_LIMIT {
                &article.body[..200]
            } else {
                &article.body[..]
            }
        };
        
        let guid = GuidBuilder::default()
            .value(link.clone())
            .build()
            .expect("Could not create article guid.");
        
        let date_posted = DateTime::<Utc>::from_utc(article.posted, Utc).to_rfc2822();
        
        let item =ItemBuilder::default()
            .title(article.title.clone())
            .link(link)
            .description(desc.to_string())
            .author(article.username.clone())
            .guid(guid)
            .pub_date(date_posted)
            .build();
            
        match item {
            Ok(i) => article_items.push(i),
            Err(e) => println!("Could not create rss article {}.  Error: {}", article.aid, e),
        }
    }
    let mut search_link = String::with_capacity(BLOG_URL.len()+10);
    search_link.push_str(BLOG_URL);
    search_link.push_str("search");
    
    let searchbox = TextInputBuilder::default()
        .title("Search")
        .name("q")
        .description("Search articles")
        .link(search_link)
        .build()
        .expect("Could not create text input item in RSS channel.");
    
    let channel_link_string: String;
    let channel_link = match custom_link {
        Some(s) => {
            channel_link_string = format!("{}/{}", BLOG_URL, s);
            // BLOG_URL
            &channel_link_string
        },
        None => {BLOG_URL},
    };
    let channel_title_string: String;
    let channel_title = match custom_title {
        Some(s) => {
            channel_title_string = format!("Vishus Blog - {}", s);
            &channel_title_string
        },
        None => { "Vishus blog" },
    };
    
    let channel = ChannelBuilder::default()
        .title(channel_title)
        .link(channel_link)
        .description("A programming and development blog about Rust, Javascript, and Web Development.")
        .language("en-us".to_string())
        .copyright("2018 Andrew Prindle".to_string())
        .ttl(720.to_string()) // half a day, 1440 minutes in a day
        .items(article_items)
        .text_input(searchbox)
        .build()
        .expect("Could not create RSS channel.");
    
    let rss_output = channel.to_string();
    let mut output = String::with_capacity(rss_output.len() + 60);
    output.push_str(r#"<?xml version="1.0"?>"#);
    output.push_str(&rss_output);
    output
}


fn rss_output(articles: Vec<Article>) -> String {
    let mut article_items: Vec<Item> = Vec::new();
    for article in &articles {
        let mut link = String::with_capacity(BLOG_URL.len()+20);
        link.push_str(BLOG_URL);
        link.push_str("article/");
        link.push_str(&article.aid.to_string());
        link.push_str("/");
        link.push_str( &encode(&article.title) );
        
        let desc: &str = if &article.description != "" {
            &article.description
        } else {
            if article.body.len() > DESC_LIMIT {
                &article.body[..200]
            } else {
                &article.body[..]
            }
        };
        
        let guid = GuidBuilder::default()
            .value(link.clone())
            .build()
            .expect("Could not create article guid.");
        
        let date_posted = DateTime::<Utc>::from_utc(article.posted, Utc).to_rfc2822();
        
        let item =ItemBuilder::default()
            .title(article.title.clone())
            .link(link)
            .description(desc.to_string())
            .author(article.username.clone())
            .guid(guid)
            .pub_date(date_posted)
            .build();
            
        match item {
            Ok(i) => article_items.push(i),
            Err(e) => println!("Could not create rss article {}.  Error: {}", article.aid, e),
        }
    }
    let mut search_link = String::with_capacity(BLOG_URL.len()+10);
    search_link.push_str(BLOG_URL);
    search_link.push_str("search");
    
    let searchbox = TextInputBuilder::default()
        .title("Search")
        .name("q")
        .description("Search articles")
        .link(search_link)
        .build()
        .expect("Could not create text input item in RSS channel.");
    
    let channel = ChannelBuilder::default()
        .title("Vishus Blog")
        .link(BLOG_URL)
        .description("A programming and development blog about Rust, Javascript, and Web Development.")
        .language("en-us".to_string())
        .copyright("2018 Andrew Prindle".to_string())
        .ttl(720.to_string()) // half a day, 1440 minutes in a day
        .items(article_items)
        .text_input(searchbox)
        .build()
        .expect("Could not create RSS channel.");
    
    let rss_output = channel.to_string();
    let mut output = String::with_capacity(rss_output.len() + 30);
    output.push_str(r#"<?xml version="1.0"?>"#);
    output.push_str(&rss_output);
    output
}


































