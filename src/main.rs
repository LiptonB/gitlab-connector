// TODO: more specific macro imports:
use serde_json::*;
use serde_derive::*;
use failure::*;
use reqwest;

//use futures::future;
// use hyper::{Body, Chunk, Method, Request, Response, Server, StatusCode};
// use hyper::rt::{Future, Stream};
// use hyper::service::service_fn;
use std::collections::HashSet;
use std::iter;
use std::str::FromStr;
use reqwest::StatusCode;
use serde_json::Value;
use failure::Error;

type Result<T> = std::result::Result<T, Error>;

// Just a simple type alias
//type BoxFut = Box<Future<Item=Response<Body>, Error=hyper::Error> + Send>;

#[derive(Debug)]
struct CIJob<'a> {
    // These are in the same order as the projects
    branch_heads: Vec<BranchHead<'a>>,
    ctx: &'a Context,
}

#[derive(Debug, PartialEq, Eq)]
enum BranchHeadType {
    Base,
    Active,
}

#[derive(Debug)]
struct BranchHead<'a> {
    commit: String,
    branch: String,
    project: &'a Project,
    ctx: &'a Context,
    head_type: BranchHeadType,
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

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    pipeline_url: String,
    extra_repo_urls: Vec<String>,
    watched_branches: Vec<String>,
    auth_token: String,
    pipeline_name: String,
    clone_method: CloneMethod,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CloneMethod {
    SSH,
    HTTP,
}

#[derive(Debug)]
struct Context {
    config: Config,
    projects: Vec<Project>,
    pipeline_repo_idx: usize,
}

#[derive(Debug)]
struct Project {
    id: u64,
    api_url: String,
    is_pipeline_repo: bool,
    clone_url: String,
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
    #[fail(display = "response missing expected keys. response: {}", response)]
    MalformedResponse {
        response: String,
    },
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
    /*
    pub fn new(pipeline_url: &str, extra_repo_urls: &[&str], watched_branches: &[&str],
        auth_token: &str, pipeline_name: &str) -> Config
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
            pipeline_name: pipeline_name.to_string(),
        }
    }
    */

    pub fn all_urls<'a>(&'a self) -> Box<dyn Iterator<Item=&'a str> + 'a> {
        Box::new(
            self.extra_repo_urls.iter().map(|s| s.as_str())
                .chain(iter::once(self.pipeline_url.as_str())))
    }

}

impl Context {
    pub fn try_new(conf: Config) -> Result<Context> {
        let mut projects = Vec::new();
        let mut pipeline_repo_idx = None;
        for (idx, url) in conf.all_urls().enumerate() {
            let resp: Value = reqwest::Client::new()
                .post(&conf.pipeline_url)
                .query(&[("private_token", &conf.auth_token)]) // replace with header?
                .send()?
                .error_for_status()?
                .json()?;

            match resp["id"].as_u64() {
                Some(id) => {
                    let is_pipeline_repo = url == conf.pipeline_url;

                    let clone_url = match conf.clone_method {
                        CloneMethod::SSH => &resp["ssh_url_to_string"],
                        CloneMethod::HTTP => &resp["http_url_to_string"],
                    };
                    let clone_url = clone_url.as_str()
                        .ok_or(ConnectorError::MalformedResponse{response: resp.to_string()})?
                        .to_string();

                    projects.push(Project{
                        id,
                        api_url: url.to_string(),
                        is_pipeline_repo,
                        clone_url,
                    });
                    if is_pipeline_repo {
                        pipeline_repo_idx = Some(idx);
                    }
                },
                _ => return Err(ConnectorError::MalformedResponse{response: resp.to_string()}.into()),
            }
        }

        match pipeline_repo_idx {
            None => return Err(
                ConnectorError::InternalError{
                    msg: "No project was the pipeline repo".to_string()}.into()),
            Some(pipeline_repo_idx) => Ok(
                Context{
                    config: conf,
                    projects,
                    pipeline_repo_idx,
                }),
        }
    }
}


impl<'a> CIJob<'a> {
    fn new(branch: &str, base: &str, ctx: &'a Context) -> Result<CIJob<'a>> {
        let mut job = CIJob {
            branch_heads: Vec::new(),
            ctx,
        };

        for project in &ctx.projects {
            let head = BranchHead::new(branch, &project, ctx, BranchHeadType::Active)?;
            if let Some(head) = head {
                job.branch_heads.push(head);
            } else {
                let head = BranchHead::new(base, &project, ctx, BranchHeadType::Base)?
                    .ok_or(ConnectorError::NonexistentBranch{branch: base.to_string()})?;
                job.branch_heads.push(head);
            }
        }

        Ok(job)
    }

    fn ensure_running(&self) -> Result<()> {
        // Check the BranchHeads for running jobs
        let mut needs_start = false;
        let mut pipelines = HashSet::new();
        for head in &self.branch_heads {
            // TODO: I wish this could be encapsulated better
            if head.head_type != BranchHeadType::Active {
                continue;
            }

            match head.get_current_pipeline(&self.ctx.config.pipeline_name)? {
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

    fn start_pipeline(&self) -> Result<()> {
        let pipeline_head = &self.branch_heads[self.ctx.pipeline_repo_idx];

        let params = json!({"ref": pipeline_head.branch, "variables": [{"key": "commits", "value": self.format_commits_str()}]});
        let start_pipeline_url = format!("{base}/pipeline", base=self.ctx.config.pipeline_url);
        let resp: Value = reqwest::Client::new()
            .post(&start_pipeline_url)
            .query(&[("private_token", &self.ctx.config.auth_token)]) // replace with header?
            .json(&params)
            .send()?
            .error_for_status()?
            .json()?;

        match (resp["id"].as_u64(), resp["status"].as_str()) {
            (Some(id), Some(status)) => self.update_statuses(id, status.parse()?),
            _ => return Err(ConnectorError::MalformedResponse{response: resp.to_string()}.into()),
        }

        Ok(())
    }

    // TODO
    fn format_commits_str(&self) -> String {
        "".to_string()
    }

    fn update_statuses(&self, _pipeline_id: u64, _pipeline_status: Status) {

    }
}

impl Pipeline {
    fn ensure_stopped(&self) {

    }
}

impl<'a> BranchHead<'a> {
    fn new(branch: &str, project: &'a Project, ctx: &'a Context, head_type: BranchHeadType)
            -> Result<Option<BranchHead<'a>>> {
        let branch_url = format!("{base}/repository/commits/{branch}", base=project.api_url, branch=branch);
        let resp = reqwest::Client::new()
            .get(&branch_url)
            .query(&[("private_token", &ctx.config.auth_token)])
            .send()?;

        if resp.status() == StatusCode::NotFound {
            return Ok(None);
        }
        let resp: Value = resp
            .error_for_status()?
            .json()?;

        match resp["id"].as_str() {
            Some(commit) => Ok(Some(BranchHead {
                branch: branch.to_string(),
                commit: commit.to_string(),
                project,
                ctx,
                head_type,
            })),
            None => Err(ConnectorError::NonexistentBranch{branch: branch.to_string()}.into()),
        }
    }

    fn get_current_pipeline(&self, name: &str) -> Result<Option<Pipeline>> {
        if !self.project.is_pipeline_repo {
            let url = format!("{base}/repository/commits/{commit}/statuses",
                            base=self.project.api_url, commit=self.commit);
            let resp: Value = reqwest::Client::new()
                .get(&url)
                .query(&[("name", name), ("private_token", &self.ctx.config.auth_token),
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
                _ => Err(ConnectorError::MalformedResponse{response: resp.to_string()}.into()),
            }
        } else {
            Err(ConnectorError::InternalError{msg: "Not implemented".to_string()}.into())
        }
    }
}

impl FromStr for Status {
    type Err = Error;

    fn from_str(status: &str) -> Result<Self> {
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
//     fn handle_mr_hook(&self) -> Result<()> {
//         let branch = "";
//         let base = "";
//         let job = CIJob::new(branch, base, &self.config)?;
//         job.ensure_running();
//         Ok(())
//     }
// 
//     fn handle_pipeline_hook(&self) -> Result<()> {
//         let branch = "";
//         let base = "";
//         // TODO: figure out whether for a base branch or a MR
//         let job = CIJob::new(branch, base, &self.config)?;
//         job.update_statuses();
//         Ok(())
//     }
// 
//     fn handle_push_hook(&self) -> Result<()> {
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
    let config_string = r#"{
        "pipeline_url": "http://192.168.56.102/api/v4/projects/4",
        "extra_repo_urls": ["http://192.168.56.102/api/v4/projects/5"],
        "watched_branches": ["master"],
        "auth_token": "xQjkvDxxpu-o2ny4YNUo",
        "pipeline_name": "gitlab-connector",
        "clone_method": "http"
    }"#;
    let config: Config = serde_json::from_str(config_string).unwrap();
    let ctx = Context::try_new(config).unwrap();

    let job = CIJob::new("other-branch", "master", &ctx).unwrap();
    job.ensure_running().unwrap();
}
