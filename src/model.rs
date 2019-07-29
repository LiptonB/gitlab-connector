use std::collections::HashSet;
use std::iter;
use std::str::FromStr;
use reqwest;
use reqwest::StatusCode;
use serde_json::{Value, json};
use serde_derive::{Serialize, Deserialize};
use failure::{Error, Fail};

type Result<T> = std::result::Result<T, Error>;

// An abstraction of a CI job testing multiple projects
#[derive(Debug)]
pub struct CIJob<'a> {
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
pub struct Config {
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
pub struct Context {
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
    pub fn from_json(config_str: &str) -> Result<Config> {
        Ok(serde_json::from_str(config_str)?)
    }

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
            None => Err(
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
    pub fn new(branch: &str, base: &str, ctx: &'a Context) -> Result<CIJob<'a>> {
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

    // Ensure that a CI pipeline has been started to test the configured commits. If any other jobs
    // are running for the same source and destination branches, stop them. When the pipeline
    // completes, add statuses to the configured commits reflecting the test results.
    pub fn ensure_running(&self) -> Result<()> {
        // Check the BranchHeads for running jobs
        let mut needs_start = false;
        let mut pipelines = HashSet::new();
        for head in &self.branch_heads {
            // TODO: I wish this could be encapsulated better
            if head.head_type != BranchHeadType::Active {
                continue;
            }

            // TODO: pretty sure this logic is just wrong
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

        if resp.status() == StatusCode::NOT_FOUND {
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
        if self.project.is_pipeline_repo {
            let url = format!("{base}/pipelines",
                            base=self.project.api_url);
            let resp: Value = reqwest::Client::new()
                .get(&url)
                .query(&[("ref", &self.branch), ("sha", &self.commit),
                         ("private_token", &self.ctx.config.auth_token)])
                .send()?
                .json()?;

            if !resp[0].is_object() {
                return Ok(None);
            }
            // TODO: web_url might not be getting returned
            match (resp[0]["web_url"].as_str(), resp[0]["status"].as_str()) {
                (Some(target_url), Some(status)) => {
                    Ok(Some(Pipeline {
                        status: status.parse()?,
                        url: target_url.to_string(),
                    }))
                },
                _ => Err(ConnectorError::MalformedResponse{response: resp.to_string()}.into()),
            }
        } else {
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
