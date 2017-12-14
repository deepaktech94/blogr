use rocket::Data;
use rocket::data::FromData;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::{Cookie, Cookies, MediaType, ContentType, Method, Status};
use rocket::Outcome;
use rocket::Outcome::Success;
use rocket::request::{FlashMessage, Form, FromForm, FormItems, FromRequest, Request};
use rocket::response::{self, Response, content, NamedFile, Redirect, Flash, Responder, Content};
use rocket::response::content::Html;
use rocket::State;
use rocket;

use std::mem;
use std::env;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::prelude::*;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::fs::{self, File};
use ::serde::{Deserialize, Serialize};

use std::sync::{Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::net::Ipv4Addr;

pub const HITS_SAVE_INTERVAL: usize = 5;

pub fn cur_dir_file(name: &str) -> PathBuf {
    if let Ok(mut dir) = env::current_exe() {
        dir.pop();
        // println!("Climbing directory tree into: {}", &dir.display());
        dir.pop();
        // println!("Loading into directory: {}", &dir.display());
        dir.set_file_name(name);
        // println!("Load file is: {}", &dir.display());
        dir
    } else {
        PathBuf::from(name)
    }
}



#[derive(Debug)]
pub struct TotalHits {
    pub total: AtomicUsize,
}


#[derive(Debug, Serialize, Deserialize)]
pub struct TotalHitsSerde {
    pub total: usize,
}



#[derive(Debug, Serialize, Deserialize)]
pub struct PageStats {
    pub map: HashMap<String, usize>,
}


#[derive(Debug, Serialize, Deserialize)]
pub struct Counter {
    pub stats: Mutex<PageStats>,
}


// Implements a Request Guard to pull data into a route
// current page/route, page views, total site hits/views
#[derive(Debug, Clone, Serialize)]
pub struct Hits(pub String, pub usize, pub usize);

// Use this for error pages to track errors
#[derive(Debug, Clone, Serialize)]
pub struct ErrorHits(pub String, pub usize, pub usize);


impl TotalHits {
    pub fn new() -> TotalHits {
        TotalHits { total: AtomicUsize::new(0) }
    }
    pub fn save(&self) {
        let filename = cur_dir_file("total_views.json");
        
        let mut f = File::create(&filename)
            .expect("Could not create file for TotalHits.");
        
        let serdes = TotalHitsSerde { total: self.total.load(Ordering::Relaxed) };
        
        // let ser: String = ::serde_json::to_string_pretty(self)
        let ser: String = ::serde_json::to_string_pretty( &serdes )
            .expect("Could not serialize TotalHits.");
        
        let bytes = f.write( ser.as_bytes() );
    }
    pub fn load() -> Self {
        let filename = cur_dir_file("total_views.json");
        let mut f_rst = File::open(&filename);
        if let Ok(mut f) = f_rst {
            let mut buffer: String = String::with_capacity(100);
            f.read_to_string(&mut buffer);
            
            // let des: Self = ::serde_json::from_str(&mut buffer)
            let des: TotalHitsSerde = ::serde_json::from_str(&mut buffer)
                .expect("Could not deserialize TotalHits from file.");
            
            let out: TotalHits = TotalHits { total: AtomicUsize::new( des.total ) };
            
            out
            // des
        } else {
            let new = TotalHits::new();
            new.save();
            new
        }
    }
}


impl Counter {
    pub fn new() -> Counter {
        Counter { stats: Mutex::new( PageStats::new() ) }
    }
    pub fn save(buffer: &str) {
        let filename = cur_dir_file("page_stats.json");
        
        let mut f = File::create(&filename)
            .expect("Could not create file for Counter.");
        
        let bytes = f.write( buffer.as_bytes() );
    }
    pub fn load() -> Counter {
        let filename = cur_dir_file("page_stats.json");
        let mut f_rst = File::open(&filename);
        if let Ok(mut f) = f_rst {
            let mut buffer: String = String::with_capacity(1000);
            f.read_to_string(&mut buffer);
            
            let page_stats = PageStats::des(buffer);
            
            Counter {
                stats: Mutex::new( page_stats ),
            }
        } else {
            let new = PageStats::new();
            let buffer = new.ser();
            Counter::save(&buffer);
            Counter {
                stats: Mutex::new( new )
            }
        }
    }
}


impl PageStats {
    pub fn new() -> PageStats {
        PageStats { map: HashMap::new() }
    }
    pub fn ser(&self) -> String {
        let ser: String = ::serde_json::to_string_pretty(self)
            .expect("Could not serialize PageStats");
        ser
    }
    pub fn des(mut buffer: String) -> Self {
        let des_rst = ::serde_json::from_str(&mut buffer);
        if let Ok(des) = des_rst {
            des
        } else {
            println!("Deserialization failed for PageStats.");
            PageStats::new()
        }
    }
}


// fn route<'a>(req: &rocket::Request<'a>) -> String {
fn route<'a>(req: &Request) -> String {
    let uri = req.uri();
    let route = uri.path();
    
    // let page = route;
    // let pagestr = page.to_string();
    // let mut page: &str = route;
    
    let mut page: &str;
    // let pagestr: String;
    
    // This first if statement allows customizable home page name in the tracking
    if route == "/" {
        page = "/";
        // pagestr = "/".to_string();
    } else if let Some(pos) = route[1..].find("/") {
        let (p, _) = route[1..].split_at(pos);
        // println!("Found route `{}`, splitting at {} to get `{}`", route, pos, p);
        if p == "article" {
            page = route;
            // pagestr = route.to_string();
        } else {
            page = p;
            // pagestr = p.to_string();
        }
    } else {
        // page = route.to_string();
        // println!("Found route: {}", route);
        page = route;
        // pagestr = route.to_string();
    }
    page.to_string()
}

fn req_guard(req: &Request, pagestr: String) -> ::rocket::request::Outcome<Hits,()> {
        // let pagestr = page.to_string();
        // let page = route(req);
        // let page = &pagestr;
        
        // let pagestr = route(req);
        
        let total_state = req.guard::<State<TotalHits>>()?;
        let mut total = total_state.total.load(Ordering::Relaxed);
        // total.wrapping_add(1);
        total += 1;
        total_state.total.store( total, Ordering::Relaxed );
        
        
        let page_views: usize;
        let ser_stats: String;
        {
            let counter = req.guard::<State<Counter>>()?;
            let mut stats = counter.stats.lock().expect("Could not unlock Counter stats mutex.");
            {
                // https://doc.rust-lang.org/std/collections/hash_map/enum.Entry.html
                let mut hits = stats.map.entry(pagestr.clone()).or_insert(0);
                *hits += 1;
                page_views = *hits;
            }
            ser_stats = stats.ser();
        }
        // (*hits).wrapping_add(1);
        // page_views = (*hits);
        if total % 10 == 0 || &pagestr == "save-hits" {
            // println!("Save interval reached. Saving page stats.");
            Counter::save(&ser_stats);
            // println!("Saved page stats, saving total hits.");
            total_state.save();
            // println!("Saved total hits.");
        }
        
        Outcome::Success( Hits(pagestr, page_views, total) )
    }


// https://rocket.rs/guide/state/#within-guards
// https://api.rocket.rs/rocket/http/uri/struct.URI.html
// impl<'a, 'r> FromRequest<'a, 'r> for PageCount {
impl<'a, 'r> FromRequest<'a, 'r> for Hits {
    type Error = ();
    
    fn from_request(req: &'a Request<'r>) -> ::rocket::request::Outcome<Hits,Self::Error> {
        req_guard(req, route(req))
    }
}


impl ErrorHits {
    pub fn error404(req: &Request) -> Hits {
        // unimplemented!()
        let route = req.uri().path();
        let prepend = "!error404";
        
        let mut uri: String = String::with_capacity(route.len() + prepend.len() + 8);
        uri.push_str(prepend);
        uri.push_str(route);
        
        // req.set_uri(uri.as_ref());
        // let hits = req.guard::<Hits>();
        
        let hits = req_guard(req, uri);
        if let Success(hitcount) = hits {
            hitcount
        } else {
            Hits(String::from("uError"), 0, 0)
        }
    }
    pub fn error500(req: &Request) -> Hits {
        // unimplemented!()
                let route = req.uri().path();
        let prepend = "!error500";
        
        let mut uri: String = String::with_capacity(route.len() + prepend.len() + 8);
        uri.push_str(prepend);
        uri.push_str(route);
        
        // req.set_uri(uri.as_ref());
        // let hits = req.guard::<Hits>();
        
        let hits = req_guard(req, uri);
        if let Success(hitcount) = hits {
            hitcount
        } else {
            Hits(String::from("uError"), 0, 0)
        }
    }
    pub fn error(req: &Request) -> Hits {
        // unimplemented!()
                let route = req.uri().path();
        let prepend = "!error";
        
        let mut uri: String = String::with_capacity(route.len() + prepend.len() + 8);
        uri.push_str(prepend);
        uri.push_str(route);
        
        // req.set_uri(uri.as_ref());
        // let hits = req.guard::<Hits>();
        
        let hits = req_guard(req, uri);
        if let Success(hitcount) = hits {
            hitcount
        } else {
            Hits(String::from("uError"), 0, 0)
        }
    }
}
































