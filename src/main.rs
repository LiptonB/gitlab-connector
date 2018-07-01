extern crate hyper;
extern crate futures;
extern crate serde_json;
extern crate reqwest;

use futures::future;
use hyper::{Body, Chunk, Method, Request, Response, Server, StatusCode};
use hyper::rt::{self, Future, Stream};
use hyper::service::service_fn;
use serde_json::Value;

// Just a simple type alias
type BoxFut = Box<Future<Item=Response<Body>, Error=hyper::Error> + Send>;

struct CIJob<'a> {
    heads: Vec<BranchHead<'a>>,
    config: &'a Config,
    client: reqwest::Client,
}

struct BranchHead<'a> {
    commit: String,
    branch: String,
    repo_url: &'a str,
    config: &'a Config,
}

struct Config {
    pipeline_url: String,
    extra_repo_urls: Vec<String>,
    watched_branches: Vec<String>,
    auth_token: String,
}

struct WebHookListener {
    config: Config,
}

impl<'a> CIJob<'a> {
    fn new(branch: &str, base: &str, conf: &'a Config) -> CIJob<'a> {
        let mut job = CIJob {
            heads: Vec::with_capacity(1+conf.extra_repo_urls.len()),
            config: conf,
            client: reqwest::Client::new(),
        };

        for url in &conf.extra_repo_urls {
            job.heads.push(BranchHead::new_with_base(branch, base, url, conf));
        }

        job
    }

    fn affects_status(&self, head: &BranchHead) -> bool {
        true
    }

    fn ensure_running(&self) {
        // Check the BranchHeads for running jobs
        //let mut ci_url = None;
        //let mut needs_start = false;
        for head in &self.heads {
            if self.affects_status(head) {
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
    }

    fn update_statuses(&self) {

    }
}

impl<'a> BranchHead<'a> {
    fn new_with_base(branch: &str, base: &str, url: &'a str, config: &'a Config) -> BranchHead<'a> {
        BranchHead {
            branch: "".to_string(),
            commit: "".to_string(),
            repo_url: url,
            config: config,
        }
    }

    fn get_status_url(&self, name: &str) -> Result<Option<String>, reqwest::Error> {
        let url = format!("{base}/repository/commits/{commit}/statuses",
                          base=self.repo_url, commit=self.commit);
        let resp: Value = reqwest::Client::new()
            .get(&url)
            .query(&[("name", name), ("private_token", &self.config.auth_token),
                     ("ref", &self.branch)])
            .send()?
            .json()?;

        Ok(resp[0]["target_url"].as_str().map(str::to_string))
    }
}

impl WebHookListener {
    fn handle_mr_hook(&self) {
        let branch = "";
        let base = "";
        let job = CIJob::new(branch, base, &self.config);
        job.ensure_running();
    }

    fn handle_pipeline_hook(&self) {
        let branch = "";
        let base = "";
        // TODO: figure out whether for a base branch or a MR
        let job = CIJob::new(branch, base, &self.config);
        job.update_statuses();
    }

    fn handle_push_hook(&self) {
        let branch = "";
        let base = "";
        // if branch in base_branches
        // TODO: tell job it's for the base branch, not a MR
        let job = CIJob::new(branch, base, &self.config);
        job.ensure_running();
        // end if
    }
}

fn call_ci(ref_id: &str) {
    // trigger = "/projects/:id/trigger/pipeline"
}

fn execute_webhook(json: &Chunk) {
    if let Ok(data) = serde_json::from_slice::<Value>(json) {
        let kind = &data["object_kind"];
        match kind.as_str() {
            Some("merge_request") => {
                println!("Was a merge request");
                if let Value::String(ref_id) = &data["object_attributes"]["last_commit"]["id"] {
                    call_ci(ref_id);
                }
            }
            _ => {}
        }
    }
}

fn webhook(req: Request<Body>) -> BoxFut {
    let mut response = Response::new(Body::empty());

    match req.method() {
        &Method::POST => {
           let future = req
               .into_body()
               .concat2()
               .map(move |json| {
                    execute_webhook(&json);
                    response
               });
           return Box::new(future);
        },
        _ => {
            *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
        },
    };

    /*
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => {
            *response.body_mut() = Body::from("Try POSTing data to /echo");
        },
        (&Method::POST, "/echo") => {
            *response.body_mut() = req.into_body();
        },
        (&Method::POST, "/echo/uppercase") => {
            // This is actually a new `futures::Stream`...
            let mapping = req
                .into_body()
                .map(|chunk| {
                    chunk.iter()
                        .map(|byte| byte.to_ascii_uppercase())
                        .collect::<Vec<u8>>()
                });

            // Use `Body::wrap_stream` to convert it to a `Body`...
            *response.body_mut() = Body::wrap_stream(mapping);
        },
        (&Method::POST, "/echo/reversed") => {
            let reversed = req
                .into_body()
                .concat2()
                .map(|chunk| {
                    let body = chunk.iter()
                        .rev()
                        .cloned()
                        .collect::<Vec<u8>>();
                    *response.body_mut() = Body::from(body);
                    response
                });
            return Box::new(reversed);
        },
        _ => {
            *response.status_mut() = StatusCode::NOT_FOUND;
        },
    };
    */

    Box::new(future::ok(response))
}

fn run_server() {
    // This is our socket address...
    let addr = ([127, 0, 0, 1], 3000).into();

    // A `Service` is needed for every connection, so this
    // creates on of our `hello_world` function.
    let new_svc = || {
        // service_fn_ok converts our function into a `Service`
        service_fn(webhook)
    };

    let server = Server::bind(&addr)
        .serve(new_svc)
        .map_err(|e| eprintln!("server error: {}", e));

    // Run this server for... forever!
    hyper::rt::run(server);
}

fn main() {
    let config = Config {
        pipeline_url: "http://192.168.56.102/api/v4/projects/4".to_string(),
        extra_repo_urls: vec!["http://192.168.56.102/api/v4/projects/5".to_string()],
        watched_branches: vec!["name".to_string()],
        auth_token: "xQjkvDxxpu-o2ny4YNUo".to_string(),
    };

    let bh = BranchHead {
        commit: "990ebddeb72329be0ffdee2898b3f3565e4291cd".to_string(),
        branch: "master".to_string(),
        repo_url: &config.pipeline_url,
        config: &config,
    };

    if let Some(status_url) = bh.get_status_url("default").unwrap() {
        println!("status_url: {}", status_url);
    } else {
        println!("No status url");
    }
}
