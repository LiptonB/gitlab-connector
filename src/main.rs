extern crate hyper;
extern crate futures;
#[macro_use] extern crate serde_json;
extern crate reqwest;
#[macro_use] extern crate failure;

//use futures::future;
// use hyper::{Body, Chunk, Method, Request, Response, Server, StatusCode};
// use hyper::rt::{Future, Stream};
// use hyper::service::service_fn;
use std::collections::HashSet;
use std::str::FromStr;
use reqwest::StatusCode;
use serde_json::Value;
use failure::Error;

// Just a simple type alias
//type BoxFut = Box<Future<Item=Response<Body>, Error=hyper::Error> + Send>;

#[derive(Debug)]
struct CIJob<'a> {
    branch_heads: Vec<BranchHead<'a>>,
    config: &'a Config,
}

#[derive(Debug)]
enum BranchHeadType {
    Base,
    Active,
}

#[derive(Debug)]
struct BranchHead<'a> {
    commit: String,
    branch: String,
    repo_url: &'a str,
    config: &'a Config,
    head_type: BranchHeadType,
    is_pipeline_repo: bool,
}

#[derive(Debug, Hash, PartialEq, Eq)]
struct Pipeline {
    status: Status,
    url: String,
}

#[derive(Debug, Hash, PartialEq, Eq)]
enum Status {
    Pending,
    Running,
    Success,
    Failed,
    Canceled,
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

#[derive(Debug, Fail)]
enum ConnectorError {
    #[fail(display = "branch does not exist: {}", branch)]
    NonexistentBranch {
        branch: String,
    },
    #[fail(display = "response missing expected keys")]
    MalformedResponse,
    #[fail(display = "invalid value for Status: {}", status)]
    InvalidStatus {
        status: String,
    },
    #[fail(display = "internal error: {}", msg)]
    InternalError {
        msg: String,
    },
}

impl Config {
    pub fn new(pipeline_url: &str, extra_repo_urls: &[&str], watched_branches: &[&str],
        auth_token: &str) -> Config
    {
        let pipeline_url = pipeline_url.to_string();
        let mut repo_urls = vec![pipeline_url.clone()];
        for &url in extra_repo_urls {
            repo_urls.push(url.to_string());
        }
        Config {
            pipeline_url,
            repo_urls,
            watched_branches: watched_branches.iter().map(|&b| b.to_string()).collect(),
            auth_token: auth_token.to_string(),
        }
    }
}

impl<'a> CIJob<'a> {
    fn new(branch: &str, base: &str, config: &'a Config) -> Result<CIJob<'a>, Error> {
        let mut job = CIJob {
            branch_heads: Vec::new(),
            config,
        };

        for url in &config.repo_urls {
            let head = BranchHead::new(branch, url, config, BranchHeadType::Active)?;
            if let Some(head) = head {
                job.branch_heads.push(head);
                continue;
            }

            let head = BranchHead::new(base, url, config, BranchHeadType::Base)?;
            let head = head.ok_or(ConnectorError::NonexistentBranch{branch: base.to_string()})?;
            job.branch_heads.push(head);
        }

        Ok(job)
    }

    fn ensure_running(&self) -> Result<(), Error> {
        // Check the BranchHeads for running jobs
        let mut needs_start = false;
        let mut pipelines = HashSet::new();
        for head in &self.branch_heads {
            match head.get_current_pipeline("default")? {
                None => {
                    needs_start = true;
                }
                Some(pipeline) => {
                    if !pipelines.contains(&pipeline) {
                        needs_start = true;
                        pipelines.insert(pipeline);
                    }
                }
            }
        }
        if needs_start {
            for pipeline in pipelines {
                pipeline.ensure_stopped();
            }
            self.start_pipeline()?;
        }
        Ok(())
    }

    fn start_pipeline(&self) -> Result<(), Error> {
        let pipeline_head = self.branch_heads
            .iter().find(|h| h.is_pipeline_repo)
            .ok_or(ConnectorError::InternalError{
                msg: "No branch head for pipeline repo".to_string()})?;

        let params = json!({"ref": pipeline_head.branch, "variables": [{"key": "commits", "value": self.format_commits_str()}]});
        let mut resp = reqwest::Client::new()
            .post(&self.config.pipeline_url)
            .query(&[("private_token", &self.config.auth_token)]) // replace with header?
            .json(&params)
            .send()?
            .error_for_status()?;

        let resp: Value = resp.json()?;
        match (resp["id"].as_i64(), resp["status"].as_str()) {
            (Some(id), Some(status)) => self.update_statuses(id, status.parse()?),
            _ => return Err(ConnectorError::MalformedResponse.into()),
        }

        Ok(())
    }

    // TODO
    fn format_commits_str(&self) -> String {
        "".to_string()
    }

    fn update_statuses(&self, pipeline_id: i64, pipeline_status: Status) {

    }
}

impl Pipeline {
    fn ensure_stopped(&self) {

    }
}

impl<'a> BranchHead<'a> {
    fn new(branch: &str, url: &'a str, config: &'a Config, head_type: BranchHeadType)
            -> Result<Option<BranchHead<'a>>, Error> {
        let branch_url = format!("{base}/repository/commits/{branch}", base=url, branch=branch);
        let resp = reqwest::Client::new()
            .get(&branch_url)
            .query(&[("private_token", &config.auth_token)])
            .send()?;

        if resp.status() == StatusCode::NotFound {
            return Ok(None);
        }
        let mut resp = resp.error_for_status()?;

        let resp: Value = resp.json()?;
        match resp["id"].as_str() {
            Some(commit) => Ok(Some(BranchHead {
                branch: branch.to_string(),
                commit: commit.to_string(),
                repo_url: url,
                config,
                head_type,
                is_pipeline_repo: url == config.pipeline_url,
            })),
            None => Err(ConnectorError::NonexistentBranch{branch: branch.to_string()}.into()),
        }
    }

    fn get_current_pipeline(&self, name: &str) -> Result<Option<Pipeline>, Error> {
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
                Ok(Some(Pipeline {
                    status: status.parse()?,
                    url: target_url.to_string(),
                }))
            },
            _ => Err(ConnectorError::MalformedResponse.into()),
        }
    }
}

impl FromStr for Status {
    type Err = Error;

    fn from_str(status: &str) -> Result<Self, Self::Err> {
        Ok(match status {
            "pending" => Status::Pending,
            "running" => Status::Running,
            "success" => Status::Success,
            "failed" => Status::Failed,
            "canceled" => Status::Canceled,
            _ => { return Err(ConnectorError::InvalidStatus{status: status.to_string()}.into()); },
        })
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
                             &vec!["http://192.168.56.102/api/v4/projects/5"],
                             &vec!["master"],
                             "xQjkvDxxpu-o2ny4YNUo");

    let job = CIJob::new("other-branch", "master", &config).unwrap();
    println!("job: {:?}", job);
    job.ensure_running().unwrap();
}
