#[cfg(feature="http-client")] extern crate hyper;
#[cfg(feature="http-client")] extern crate url;
extern crate serde_json;

use std::cmp::PartialEq;
use std::collections::BTreeMap;
use std::io;
use std::result;

pub mod images;
pub mod http;
pub use images::Iter as ImagesIter;

const LANGUAGE_URL_MARKER:&'static str = "{language}";

macro_rules! results {
    ($data: expr, $query_field: expr) => {
        // There has to be a better way to write the following code
        try!(
            $data.as_object()
            .and_then(|x| x.get("query"))
            .and_then(|x| x.as_object())
            .and_then(|x| x.get($query_field))
            .and_then(|x| x.as_array())
            .ok_or(Error::JSONPathError)
            ).into_iter().filter_map(|i|
                i.as_object()
                .and_then(|i| i.get("title"))
                .and_then(|s| s.as_string().map(|s| s.to_owned()))
                ).collect()
    }
}

macro_rules! cont {
    ($this: expr, $cont: expr, $($params: expr),*) => {{
        let qp = $this.identifier.query_param();
        let mut params = vec![
            $($params),*,
            ("format", "json"),
            ("action", "query"),
            (&*qp.0, &*qp.1),
        ];
        match *$cont {
            Some(ref v) => {
                for x in v.iter() { params.push((&*x.0, &*x.1)); }
            },
            None => params.push(("continue", "")),
        }
        let q = try!($this.wikipedia.query(params.into_iter()));

        let pages = try!(q
            .as_object()
            .and_then(|x| x.get("query"))
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("pages"))
            .and_then(|x| x.as_object())
            .ok_or(Error::JSONPathError));
        Ok((pages.clone(), try!($this.parse_cont(&q))))
    }}
}


#[derive(Debug)]
pub enum Error {
    HTTPError,
    IOError(io::Error),
    JSONError(serde_json::error::Error),
    JSONPathError,
    InvalidParameter(String),
}

impl std::convert::From<http::Error> for Error {
    fn from(_: http::Error) -> Self {
        Error::HTTPError
    }
}

impl std::convert::From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::IOError(e)
    }
}

impl std::convert::From<serde_json::error::Error> for Error {
    fn from(e: serde_json::error::Error) -> Self {
        Error::JSONError(e)
    }
}

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub struct Wikipedia<A: http::HttpClient> {
    pub client: A,
    pub pre_language_url:String,
    pub post_language_url:String,
    pub language:String,
    pub search_results:u32,
    pub images_results:String,
}

impl<A: http::HttpClient + Default> Default for Wikipedia<A> {
    fn default() -> Self {
        let mut client = A::default();
        client.user_agent("wikipedia (https://github.com/seppo0010/wikipedia-rs)".to_owned());
        Wikipedia {
            client: client,
            pre_language_url: "https://".to_owned(),
            post_language_url: ".wikipedia.org/w/api.php".to_owned(),
            language: "en".to_owned(),
            search_results: 10,
            images_results: "max".to_owned(),
        }
    }
}

impl<A: http::HttpClient> Wikipedia<A> {
    pub fn base_url(&self) -> String {
        format!("{}{}{}", self.pre_language_url, self.language, self.post_language_url)
    }

    pub fn set_base_url(&mut self, base_url: &str) {
        let index = match base_url.find(LANGUAGE_URL_MARKER) {
            Some(i) => i,
            None => {
                self.pre_language_url = base_url.to_owned();
                self.language = "".to_owned();
                self.post_language_url = "".to_owned();
                return;
            }
        };
        self.pre_language_url = base_url[0..index].to_owned();
        self.post_language_url = base_url[index+LANGUAGE_URL_MARKER.len()..].to_owned();
    }

    fn query<'a, I>(&self, args: I) -> Result<serde_json::Value>
            where I: Iterator<Item=(&'a str, &'a str)> {
        let response_str = try!(self.client.get(&*self.base_url(), args));
        Ok(try!(serde_json::from_str(&*response_str)))
    }

    pub fn search(&self, query: &str) -> Result<Vec<String>> {
        let results = &*format!("{}", self.search_results);
        let data = try!(self.query(vec![
                ("list", "search"),
                ("srprop", ""),
                ("srlimit", results),
                ("srsearch", query),
                ("format", "json"),
                ("action", "query"),
            ].into_iter()));

        Ok(results!(data, "search"))
    }

    pub fn geosearch(&self, latitude: f64, longitude: f64, radius: u16) -> Result<Vec<String>> {
        if latitude < -90.0 || latitude > 90.0 {
            return Err(Error::InvalidParameter("latitude".to_string()))
        }
        if longitude < -180.0 || longitude > 180.0 {
            return Err(Error::InvalidParameter("longitude".to_string()))
        }
        if radius < 10 || radius > 10000 {
            return Err(Error::InvalidParameter("radius".to_string()))
        }
        let results = &*format!("{}", self.search_results);
        let data = try!(self.query(vec![
                ("list", "geosearch"),
                ("gsradius", &*format!("{}", radius)),
                ("gscoord", &*format!("{}|{}", latitude, longitude)),
                ("gslimit", results),
                ("format", "json"),
                ("action", "query"),
                ].into_iter()));
        Ok(results!(data, "geosearch"))
    }

    pub fn random_count(&self, count: u8) -> Result<Vec<String>> {
        let data = try!(self.query(vec![
                ("list", "random"),
                ("rnnamespace", "0"),
                ("rnlimit", &*format!("{}", count)),
                ("format", "json"),
                ("action", "query"),
                ].into_iter()));
        let r:Vec<String> = results!(data, "random");
        Ok(r)
    }

    pub fn random(&self) -> Result<Option<String>> {
        Ok(try!(self.random_count(1)).into_iter().next())
    }

    pub fn page_from_title<'a>(&'a self, title: String) -> Page<'a, A> {
        Page::from_title(self, title)
    }

    pub fn page_from_pageid<'a>(&'a self, pageid: String) -> Page<'a, A> {
        Page::from_pageid(self, pageid)
    }
}

#[derive(Debug)]
enum TitlePageId {
    Title(String),
    PageId(String),
}

impl TitlePageId {
    fn query_param(&self) -> (String, String) {
        match *self {
            TitlePageId::Title(ref s) => ("titles".to_owned(), s.clone()),
            TitlePageId::PageId(ref s) => ("pageids".to_owned(), s.clone()),
        }
    }
}

#[derive(Debug)]
pub struct Page<'a, A: 'a + http::HttpClient> {
    wikipedia: &'a Wikipedia<A>,
    identifier: TitlePageId,
}

impl<'a, A: http::HttpClient> Page<'a, A> {
    pub fn from_title(wikipedia: &'a Wikipedia<A>, title: String) -> Page<A> {
        Page { wikipedia: wikipedia, identifier: TitlePageId::Title(title) }
    }

    pub fn from_pageid(wikipedia: &'a Wikipedia<A>, pageid: String) -> Page<A> {
        Page { wikipedia: wikipedia, identifier: TitlePageId::PageId(pageid) }
    }

    fn redirect(&self, q: &serde_json::Value) -> Option<String> {
        q.as_object()
            .and_then(|x| x.get("query"))
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("redirects"))
            .and_then(|x| x.as_array())
            .and_then(|x| x.into_iter().next())
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("to"))
            .and_then(|x| x.as_string())
            .map(|x| x.to_owned())
    }

    pub fn get_content(&self) -> Result<String> {
        let qp = self.identifier.query_param();
        let q = try!(self.wikipedia.query(vec![
            ("prop", "extracts|revisions"),
            ("explaintext", ""),
            ("rvprop", "ids"),
            ("redirects", ""),
            ("format", "json"),
            ("action", "query"),
            (&*qp.0, &*qp.1),
        ].into_iter()));

        match self.redirect(&q) {
            Some(r) => return Page::from_title(&self.wikipedia, r).get_content(),
            None => (),
        }
        let pages = try!(q
            .as_object()
            .and_then(|x| x.get("query"))
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("pages"))
            .and_then(|x| x.as_object())
            .ok_or(Error::JSONPathError));
        let pageid = match pages.keys().next() {
            Some(p) => p,
            None => return Err(Error::JSONPathError),
        };
        Ok(try!(pages.get(pageid)
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("extract"))
            .and_then(|x| x.as_string())
            .ok_or(Error::JSONPathError))
            .to_owned())
    }

    pub fn get_html_content(&self) -> Result<String> {
        let qp = self.identifier.query_param();
        let q = try!(self.wikipedia.query(vec![
            ("prop", "revisions"),
            ("rvprop", "content"),
            ("rvlimit", "1"),
            ("rvparse", ""),
            ("redirects", ""),
            ("format", "json"),
            ("action", "query"),
            (&*qp.0, &*qp.1),
        ].into_iter()));

        match self.redirect(&q) {
            Some(r) => return Page::from_title(&self.wikipedia, r).get_html_content(),
            None => (),
        }
        let pages = try!(q
            .as_object()
            .and_then(|x| x.get("query"))
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("pages"))
            .and_then(|x| x.as_object())
            .ok_or(Error::JSONPathError));
        let pageid = match pages.keys().next() {
            Some(p) => p,
            None => return Err(Error::JSONPathError),
        };
        Ok(try!(pages.get(pageid)
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("revisions"))
            .and_then(|x| x.as_array())
            .and_then(|x| x.into_iter().next())
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("*"))
            .and_then(|x| x.as_string())
            .ok_or(Error::JSONPathError))
            .to_owned())
    }

    pub fn get_summary(&self) -> Result<String> {
        let qp = self.identifier.query_param();
        let q = try!(self.wikipedia.query(vec![
            ("prop", "extracts"),
            ("explaintext", ""),
            ("exintro", ""),
            ("redirects", ""),
            ("format", "json"),
            ("action", "query"),
            (&*qp.0, &*qp.1),
        ].into_iter()));

        match self.redirect(&q) {
            Some(r) => return Page::from_title(&self.wikipedia, r).get_summary(),
            None => (),
        }
        let pages = try!(q
            .as_object()
            .and_then(|x| x.get("query"))
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("pages"))
            .and_then(|x| x.as_object())
            .ok_or(Error::JSONPathError));
        let pageid = match pages.keys().next() {
            Some(p) => p,
            None => return Err(Error::JSONPathError),
        };
        Ok(try!(pages.get(pageid)
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("extract"))
            .and_then(|x| x.as_string())
            .ok_or(Error::JSONPathError))
            .to_owned())
    }

    fn parse_cont(&self, q: &serde_json::Value) -> Result<Option<Vec<(String, String)>>> {
        let cont = match q
            .as_object()
            .and_then(|x| x.get("continue"))
            .and_then(|x| x.as_object()) {
            Some(v) => v,
            None => return Ok(None),
        };
        let mut cont_v = vec![];
        for (k, v) in cont.into_iter() {
            let value = try!(v.as_string().ok_or(Error::JSONPathError));
            cont_v.push((k.clone(), value.to_owned()));
        }
        Ok(Some(cont_v))
    }

    fn request_images(&self, cont: &Option<Vec<(String, String)>>) ->
            Result<(BTreeMap<String, serde_json::Value>, Option<Vec<(String, String)>>)> {
        cont!(self, cont,
            ("generator", "images"),
            ("gimlimit", &*self.wikipedia.images_results),
            ("prop", "imageinfo"),
            ("iiprop", "url")
        )
    }

    pub fn get_images(&self) -> Result<ImagesIter<A>> {
        ImagesIter::new(&self)
    }

    pub fn get_coordinates(&self) -> Result<Option<(f64, f64)>> {
        let qp = self.identifier.query_param();
        let params = vec![
            ("prop", "coordinates"),
            ("colimit", "max"),
            ("redirects", ""),
            ("format", "json"),
            ("action", "query"),
            (&*qp.0, &*qp.1),
        ];
        let q = try!(self.wikipedia.query(params.into_iter()));

        match self.redirect(&q) {
            Some(r) => return Page::from_title(&self.wikipedia, r).get_coordinates(),
            None => (),
        }
        let pages = try!(q
            .as_object()
            .and_then(|x| x.get("query"))
            .and_then(|x| x.as_object())
            .and_then(|x| x.get("pages"))
            .and_then(|x| x.as_object())
            .ok_or(Error::JSONPathError));
        let pageid = match pages.keys().next() {
            Some(p) => p,
            None => return Err(Error::JSONPathError),
        };
        let coord = match pages.get(pageid)
                .and_then(|x| x.as_object())
                .and_then(|x| x.get("coordinates"))
                .and_then(|x| x.as_array())
                .and_then(|x| x.into_iter().next())
                .and_then(|x| x.as_object()) {
            Some(c) => c,
            None => return Ok(None),
        };
        Ok(Some((
            try!(coord.get("lat").and_then(|x| x.as_f64()).ok_or(Error::JSONPathError)),
            try!(coord.get("lon").and_then(|x| x.as_f64()).ok_or(Error::JSONPathError)),
        )))
    }
}

impl<'a, A: http::HttpClient> PartialEq<Page<'a, A>> for Page<'a, A> {
    fn eq(&self, other: &Page<A>) -> bool {
        match self.identifier {
            TitlePageId::Title(ref t1) => match other.identifier {
                TitlePageId::Title(ref t2) => t1 == t2,
                TitlePageId::PageId(_) => false,
            },
            TitlePageId::PageId(ref p1) => match other.identifier {
                TitlePageId::Title(_) => false,
                TitlePageId::PageId(ref p2) => p1 == p2,
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::Wikipedia;
    use super::http::HttpClient;
    use super::images;
    use std::sync::Mutex;

    struct MockClient {
        pub url: Mutex<Vec<String>>,
        pub user_agent: Option<String>,
        pub arguments: Mutex<Vec<Vec<(String, String)>>>,
        pub response: Mutex<Vec<String>>,
    }

    impl Default for MockClient {
        fn default() -> Self {
            MockClient {
                url: Mutex::new(Vec::new()),
                user_agent: None,
                arguments: Mutex::new(Vec::new()),
                response: Mutex::new(Vec::new()),
            }
        }
    }

    impl super::http::HttpClient for MockClient {
        fn user_agent(&mut self, user_agent: String) {
            self.user_agent = Some(user_agent)
        }

        fn get<'a, I>(&self, base_url: &str, args: I) -> Result<String, super::http::Error>
                where I: Iterator<Item=(&'a str, &'a str)> {
            self.url.lock().unwrap().push(base_url.to_owned());
            self.arguments.lock().unwrap().push(args.map(|x| (x.0.to_owned(), x.1.to_owned())).collect());
            Ok(self.response.lock().unwrap().remove(0))
        }
    }

    #[test]
    fn base_url() {
        let mut wikipedia = Wikipedia::<MockClient>::default();
        assert_eq!(wikipedia.base_url(), "https://en.wikipedia.org/w/api.php");
        wikipedia.language = "es".to_owned();
        assert_eq!(wikipedia.base_url(), "https://es.wikipedia.org/w/api.php");

        wikipedia.set_base_url("https://hello.{language}.world/");
        assert_eq!(wikipedia.base_url(), "https://hello.es.world/");

        wikipedia.set_base_url("https://hello.world/");
        assert_eq!(wikipedia.base_url(), "https://hello.world/");
    }

    #[test]
    fn user_agent() {
        let mut wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{}".to_owned());
        wikipedia.search("hello world").unwrap_err();
        assert_eq!(&*wikipedia.client.user_agent.unwrap(), "wikipedia (https://github.com/seppo0010/wikipedia-rs)");

        let mut client = MockClient::default();
        client.user_agent("hello world".to_owned());
        client.response.lock().unwrap().push("{}".to_owned());
        wikipedia.client = client;
        wikipedia.search("hello world").unwrap_err();
        assert_eq!(&*wikipedia.client.user_agent.unwrap(), "hello world");
    }

    #[test]
    fn search() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"search\":[{\"title\":\"hello\"}, {\"title\":\"world\"}]}}".to_owned());
        assert_eq!(
                wikipedia.search("hello world").unwrap(),
                vec![
                "hello".to_owned(),
                "world".to_owned(),
                ]);
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec!["https://en.wikipedia.org/w/api.php".to_owned()]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![vec![
                    ("list".to_owned(), "search".to_owned()),
                    ("srprop".to_owned(), "".to_owned()),
                    ("srlimit".to_owned(), "10".to_owned()),
                    ("srsearch".to_owned(), "hello world".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned())
                    ]]);
    }

    #[test]
    fn geosearch() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"geosearch\":[{\"title\":\"hello\"}, {\"title\":\"world\"}]}}".to_owned());
        assert_eq!(
                wikipedia.geosearch(-34.603333, -58.381667, 10).unwrap(),
                vec![
                "hello".to_owned(),
                "world".to_owned(),
                ]);
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec!["https://en.wikipedia.org/w/api.php".to_owned()]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![vec![
                    ("list".to_owned(), "geosearch".to_owned()),
                    ("gsradius".to_owned(), "10".to_owned()),
                    ("gscoord".to_owned(), "-34.603333|-58.381667".to_owned()),
                    ("gslimit".to_owned(), "10".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned())
                    ]]);
    }

    #[test]
    fn random_count() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"random\":[{\"title\":\"hello\"}, {\"title\":\"world\"}]}}".to_owned());
        assert_eq!(
                wikipedia.random_count(10).unwrap(),
                vec![
                "hello".to_owned(),
                "world".to_owned(),
                ]);
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec!["https://en.wikipedia.org/w/api.php".to_owned()]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![vec![
                    ("list".to_owned(), "random".to_owned()),
                    ("rnnamespace".to_owned(), "0".to_owned()),
                    ("rnlimit".to_owned(), "10".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned())
                    ]]);
    }

    #[test]
    fn random() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"random\":[{\"title\":\"hello\"}, {\"title\":\"world\"}]}}".to_owned());
        assert_eq!(
                wikipedia.random().unwrap(),
                Some("hello".to_owned())
                );
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec!["https://en.wikipedia.org/w/api.php".to_owned()]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![vec![
                    ("list".to_owned(), "random".to_owned()),
                    ("rnnamespace".to_owned(), "0".to_owned()),
                    ("rnlimit".to_owned(), "1".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned())
                    ]]);
    }

    #[test]
    fn page_content() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"pages\":{\"a\":{\"extract\":\"hello\"}}}}".to_owned());
        let page = wikipedia.page_from_pageid("4138548".to_owned());
        let html = page.get_content().unwrap();
        assert_eq!(
                html,
                "hello".to_owned()
                );
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec!["https://en.wikipedia.org/w/api.php".to_owned()]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![vec![
                    ("prop".to_owned(), "extracts|revisions".to_owned()),
                    ("explaintext".to_owned(), "".to_owned()),
                    ("rvprop".to_owned(), "ids".to_owned()),
                    ("redirects".to_owned(), "".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned()),
                    ("pageids".to_owned(), "4138548".to_owned()),
                    ]]);
    }

    #[test]
    fn page_html_content() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"pages\":{\"a\":{\"revisions\":[{\"*\":\"hello\"}]}}}}".to_owned());
        let page = wikipedia.page_from_pageid("4138548".to_owned());
        let html = page.get_html_content().unwrap();
        assert_eq!(
                html,
                "hello".to_owned()
                );
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec!["https://en.wikipedia.org/w/api.php".to_owned()]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![vec![
                    ("prop".to_owned(), "revisions".to_owned()),
                    ("rvprop".to_owned(), "content".to_owned()),
                    ("rvlimit".to_owned(), "1".to_owned()),
                    ("rvparse".to_owned(), "".to_owned()),
                    ("redirects".to_owned(), "".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned()),
                    ("pageids".to_owned(), "4138548".to_owned()),
                    ]]);
    }

    #[test]
    fn page_summary() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"pages\":{\"a\":{\"extract\":\"hello\"}}}}".to_owned());
        let page = wikipedia.page_from_title("Parkinson's law of triviality".to_owned());
        let summary = page.get_summary().unwrap();
        assert_eq!(
                summary,
                "hello".to_owned()
                );
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec!["https://en.wikipedia.org/w/api.php".to_owned()]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![vec![
                    ("prop".to_owned(), "extracts".to_owned()),
                    ("explaintext".to_owned(), "".to_owned()),
                    ("exintro".to_owned(), "".to_owned()),
                    ("redirects".to_owned(), "".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned()),
                    ("titles".to_owned(), "Parkinson\'s law of triviality".to_owned())
                    ]]);
    }

    #[test]
    fn page_redirect_summary() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"redirects\":[{\"to\":\"hello world\"}]}}".to_owned());
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"pages\":{\"a\":{\"extract\":\"hello\"}}}}".to_owned());
        let page = wikipedia.page_from_title("Parkinson's law of triviality".to_owned());
        let summary = page.get_summary().unwrap();
        assert_eq!(
                summary,
                "hello".to_owned()
                );
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec![
                "https://en.wikipedia.org/w/api.php".to_owned(),
                "https://en.wikipedia.org/w/api.php".to_owned(),
                ]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![
                vec![
                    ("prop".to_owned(), "extracts".to_owned()),
                    ("explaintext".to_owned(), "".to_owned()),
                    ("exintro".to_owned(), "".to_owned()),
                    ("redirects".to_owned(), "".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned()),
                    ("titles".to_owned(), "Parkinson\'s law of triviality".to_owned())
                ],
                vec![
                    ("prop".to_owned(), "extracts".to_owned()),
                    ("explaintext".to_owned(), "".to_owned()),
                    ("exintro".to_owned(), "".to_owned()),
                    ("redirects".to_owned(), "".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned()),
                    ("titles".to_owned(), "hello world".to_owned())
                    ]
                ]
                );
    }

    #[test]
    fn page_images() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"continue\": {\"lol\":\"1\"},\"query\":{\"pages\":{\"a\":{\"title\":\"Image 1\", \"imageinfo\":[{\"url\": \"http://example.com/image1.jpg\", \"descriptionurl\": \"http://example.com/image1.jpg.html\"}]}}}}".to_owned());
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"pages\":{\"a\":{\"title\":\"Image 2\", \"imageinfo\":[{\"url\": \"http://example.com/image2.jpg\", \"descriptionurl\": \"http://example.com/image2.jpg.html\"}]}}}}".to_owned());
        let page = wikipedia.page_from_title("Parkinson's law of triviality".to_owned());
        let images = page.get_images().unwrap().collect::<Vec<_>>();
        assert_eq!(
                images,
                vec![
                images::Image {
                    url: "http://example.com/image1.jpg".to_owned(),
                    title: "Image 1".to_owned(),
                    description_url: "http://example.com/image1.jpg.html".to_owned(),
                },
                images::Image {
                    url: "http://example.com/image2.jpg".to_owned(),
                    title: "Image 2".to_owned(),
                    description_url: "http://example.com/image2.jpg.html".to_owned(),
                }
                ]);
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec![
                "https://en.wikipedia.org/w/api.php".to_owned(),
                "https://en.wikipedia.org/w/api.php".to_owned(),
                ]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![
                vec![
                    ("generator".to_owned(), "images".to_owned()),
                    ("gimlimit".to_owned(), "max".to_owned()),
                    ("prop".to_owned(), "imageinfo".to_owned()),
                    ("iiprop".to_owned(), "url".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned()),
                    ("titles".to_owned(), "Parkinson\'s law of triviality".to_owned()),
                    ("continue".to_owned(), "".to_owned())
                ],
                vec![
                    ("generator".to_owned(), "images".to_owned()),
                    ("gimlimit".to_owned(), "max".to_owned()),
                    ("prop".to_owned(), "imageinfo".to_owned()),
                    ("iiprop".to_owned(), "url".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned()),
                    ("titles".to_owned(), "Parkinson\'s law of triviality".to_owned()),
                    ("lol".to_owned(), "1".to_owned())
                ]
                ]
                );
    }

    #[test]
    fn page_coordinates() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"pages\":{\"a\":{\"coordinates\":[{\"lat\": 2.1, \"lon\":-1.3}]}}}}".to_owned());
        let page = wikipedia.page_from_title("World".to_owned());
        let coordinates = page.get_coordinates().unwrap().unwrap();
        assert_eq!(
                coordinates,
                (2.1, -1.3)
                );
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec!["https://en.wikipedia.org/w/api.php".to_owned()]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![vec![
                    ("prop".to_owned(), "coordinates".to_owned()),
                    ("colimit".to_owned(), "max".to_owned()),
                    ("redirects".to_owned(), "".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned()),
                    ("titles".to_owned(), "World".to_owned())
                    ]]);
    }

    #[test]
    fn page_no_coordinates() {
        let wikipedia = Wikipedia::<MockClient>::default();
        wikipedia.client.response.lock().unwrap().push("{\"query\":{\"pages\":{\"a\":{}}}}".to_owned());
        let page = wikipedia.page_from_title("World".to_owned());
        assert!(page.get_coordinates().unwrap().is_none());
        assert_eq!(*wikipedia.client.url.lock().unwrap(),
                vec!["https://en.wikipedia.org/w/api.php".to_owned()]);
        assert_eq!(*wikipedia.client.arguments.lock().unwrap(),
                vec![vec![
                    ("prop".to_owned(), "coordinates".to_owned()),
                    ("colimit".to_owned(), "max".to_owned()),
                    ("redirects".to_owned(), "".to_owned()),
                    ("format".to_owned(), "json".to_owned()),
                    ("action".to_owned(), "query".to_owned()),
                    ("titles".to_owned(), "World".to_owned())
                    ]]);
    }
}
