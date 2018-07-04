extern crate hyper;
extern crate futures;
extern crate serde_json;
extern crate reqwest;

//use futures::future;
// use hyper::{Body, Chunk, Method, Request, Response, Server, StatusCode};
// use hyper::rt::{Future, Stream};
// use hyper::service::service_fn;
use reqwest::StatusCode;
use serde_json::Value;
use std::error::Error as StdError; // needed for `description`

// Just a simple type alias
//type BoxFut = Box<Future<Item=Response<Body>, Error=hyper::Error> + Send>;

#[derive(Debug)]
struct CIJob<'a> {
    branch_heads: Vec<BranchHead<'a>>,
    base_heads: Vec<BranchHead<'a>>,
    config: &'a Config,
}

#[derive(Debug)]
struct BranchHead<'a> {
    commit: String,
    branch: String,
    repo_url: &'a str,
    config: &'a Config,
}

#[derive(Debug)]
struct Config {
    pipeline_url: String,
    repo_urls: Vec<String>,
    watched_branches: Vec<String>,
    auth_token: String,
}

// struct WebHookListener {
//     config: Config,
// }

#[derive(Debug)]
struct Error {
    msg: String,
}

impl Config {
    pub fn new<'i, I>(pipeline_url: &str, extra_repo_urls: I, watched_branches: I,
           auth_token: &str) -> Config
    where
        I: Iterator<Item = &'i str>,
    {
        let pipeline_url = pipeline_url.to_owned();
        let mut repo_urls = vec![pipeline_url.clone()];
        for url in extra_repo_urls {
            repo_urls.push(url.to_owned());
        }
        Config {
            pipeline_url,
            repo_urls,
            watched_branches: watched_branches.map(&str::to_owned).collect(),
            auth_token: auth_token.to_owned(),
        }
    }
}

impl<'a> CIJob<'a> {
    fn new(branch: &str, base: &str, conf: &'a Config) -> Result<CIJob<'a>, Error> {
        let mut job = CIJob {
            branch_heads: Vec::new(),
            base_heads: Vec::new(),
            config: conf,
        };

        for url in &conf.repo_urls {
            let head = BranchHead::new(branch, url, conf)?;
            if let Some(head) = head {
                job.branch_heads.push(head);
                continue;
            }

            let head = BranchHead::new(base, url, conf)?;
            let head = head.ok_or(Error::from("Base branch does not exist"))?;
            job.base_heads.push(head);
        }

        Ok(job)
    }

    fn ensure_running(&self) -> Result<(), Error> {
        // Check the BranchHeads for running jobs
        //let mut ci_url = None;
        //let mut needs_start = false;
        for head in &self.branch_heads {
            if let Some((status, url)) = head.get_status_url("default")? {
                println!("status: {}", status);
                println!("url: {}", url);
            }

            /*
            let result = head.get_status("MR-CI")?;
            match result {
                None => {
                    needs_start = true;
                    break;
                }
                Some(status_url) => {
                    match ci_url {
                        None => {
                            ci_url = result;
                        }
                        Some(ci_url_val) => {
                            if status_url != ci_url_val {
                                needs_start = true;
                                break;
                            }
                        }
                    }
                }
            }
            */
        }
        // Check if already running
        // GET http://192.168.56.102/api/v4/projects/:id/pipelines
        //
        // For now, just make the call to the trigger_url
        // POST http://192.168.56.102/api/v4/projects/:id/pipeline
        // ref=
        // variables=
        //let params = [("token", &self.config.trigger_token[..]), ("ref", "master")];
        //let res = self.client.post(&self.config.trigger_url)
        //    .form(&params)
        //    .send();
        Ok(())
    }

    fn update_statuses(&self) {

    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Error {
        Error::from(err.description())
    }
}

impl<'a> From<&'a str> for Error {
    fn from(msg: &str) -> Error {
        Error {msg: msg.to_string()}
    }
}

impl<'a> BranchHead<'a> {
    fn new(branch: &str, url: &'a str, config: &'a Config)
            -> Result<Option<BranchHead<'a>>, Error> {
        let branch_url = format!("{base}/repository/commits/{branch}", base=url, branch=branch);
        let mut resp = reqwest::Client::new()
            .get(&branch_url)
            .query(&[("private_token", &config.auth_token)])
            .send()?;

        if resp.status() == StatusCode::NotFound {
            return Ok(None);
        }

        let resp: Value = resp.json()?;
        match resp["id"].as_str() {
            Some(commit) => Ok(Some(BranchHead {
                branch: branch.to_string(),
                commit: commit.to_string(),
                repo_url: url,
                config,
            })),
            None => Err(Error::from("Branch not found")),
        }
    }

    fn get_status_url(&self, name: &str) -> Result<Option<(String, String)>, Error> {
        let url = format!("{base}/repository/commits/{commit}/statuses",
                          base=self.repo_url, commit=self.commit);
        let resp: Value = reqwest::Client::new()
            .get(&url)
            .query(&[("name", name), ("private_token", &self.config.auth_token),
                     ("ref", &self.branch)])
            .send()?
            .json()?;

        if !resp[0].is_object() {
            return Ok(None);
        }
        match (resp[0]["target_url"].as_str(), resp[0]["status"].as_str()) {
            (Some(target_url), Some(status)) => {
                Ok(Some((status.to_string(), target_url.to_string())))
            },
            _ => Err(Error::from("Response missing target_url or status field")),
        }
    }
}

// impl WebHookListener {
//     fn handle_mr_hook(&self) -> Result<(), Error> {
//         let branch = "";
//         let base = "";
//         let job = CIJob::new(branch, base, &self.config)?;
//         job.ensure_running();
//         Ok(())
//     }
// 
//     fn handle_pipeline_hook(&self) -> Result<(), Error> {
//         let branch = "";
//         let base = "";
//         // TODO: figure out whether for a base branch or a MR
//         let job = CIJob::new(branch, base, &self.config)?;
//         job.update_statuses();
//         Ok(())
//     }
// 
//     fn handle_push_hook(&self) -> Result<(), Error> {
//         let branch = "";
//         let base = "";
//         // if branch in base_branches
//         // TODO: tell job it's for the base branch, not a MR
//         let job = CIJob::new(branch, base, &self.config)?;
//         job.ensure_running();
//         Ok(())
//         // end if
//     }
// }
// 
// fn call_ci(ref_id: &str) {
//     // trigger = "/projects/:id/trigger/pipeline"
// }
// 
// fn execute_webhook(json: &Chunk) {
//     if let Ok(data) = serde_json::from_slice::<Value>(json) {
//         let kind = &data["object_kind"];
//         match kind.as_str() {
//             Some("merge_request") => {
//                 println!("Was a merge request");
//                 if let Value::String(ref_id) = &data["object_attributes"]["last_commit"]["id"] {
//                     call_ci(ref_id);
//                 }
//             }
//             _ => {}
//         }
//     }
// }
// 
// fn webhook(req: Request<Body>) -> BoxFut {
//     let mut response = Response::new(Body::empty());
// 
//     match req.method() {
//         &Method::POST => {
//            let future = req
//                .into_body()
//                .concat2()
//                .map(move |json| {
//                     execute_webhook(&json);
//                     response
//                });
//            return Box::new(future);
//         },
//         _ => {
//             *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
//         },
//     };
// 
//     /*
//     match (req.method(), req.uri().path()) {
//         (&Method::GET, "/") => {
//             *response.body_mut() = Body::from("Try POSTing data to /echo");
//         },
//         (&Method::POST, "/echo") => {
//             *response.body_mut() = req.into_body();
//         },
//         (&Method::POST, "/echo/uppercase") => {
//             // This is actually a new `futures::Stream`...
//             let mapping = req
//                 .into_body()
//                 .map(|chunk| {
//                     chunk.iter()
//                         .map(|byte| byte.to_ascii_uppercase())
//                         .collect::<Vec<u8>>()
//                 });
// 
//             // Use `Body::wrap_stream` to convert it to a `Body`...
//             *response.body_mut() = Body::wrap_stream(mapping);
//         },
//         (&Method::POST, "/echo/reversed") => {
//             let reversed = req
//                 .into_body()
//                 .concat2()
//                 .map(|chunk| {
//                     let body = chunk.iter()
//                         .rev()
//                         .cloned()
//                         .collect::<Vec<u8>>();
//                     *response.body_mut() = Body::from(body);
//                     response
//                 });
//             return Box::new(reversed);
//         },
//         _ => {
//             *response.status_mut() = StatusCode::NOT_FOUND;
//         },
//     };
//     */
// 
//     Box::new(future::ok(response))
// }
// 
// fn run_server() {
//     // This is our socket address...
//     let addr = ([127, 0, 0, 1], 3000).into();
// 
//     // A `Service` is needed for every connection, so this
//     // creates on of our `hello_world` function.
//     let new_svc = || {
//         // service_fn_ok converts our function into a `Service`
//         service_fn(webhook)
//     };
// 
//     let server = Server::bind(&addr)
//         .serve(new_svc)
//         .map_err(|e| eprintln!("server error: {}", e));
// 
//     // Run this server for... forever!
//     hyper::rt::run(server);
// }

fn main() {
    let config = Config::new("http://192.168.56.102/api/v4/projects/4",
                             vec!["http://192.168.56.102/api/v4/projects/5"].into_iter(),
                             vec!["master"].into_iter(),
                             "xQjkvDxxpu-o2ny4YNUo");

    let job = CIJob::new("other-branch", "master", &config).unwrap();
    println!("job: {:?}", job);
    job.ensure_running();
}
