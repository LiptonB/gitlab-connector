mod model;
mod server;

fn main() {
    let config_string = r#"{
        "pipeline_url": "http://192.168.56.102/api/v4/projects/4",
        "extra_repo_urls": ["http://192.168.56.102/api/v4/projects/5"],
        "watched_branches": ["master"],
        "auth_token": "xQjkvDxxpu-o2ny4YNUo",
        "pipeline_name": "gitlab-connector",
        "clone_method": "http"
    }"#;
    let config = model::Config::from_json(config_string).unwrap();
    let ctx = model::Context::try_new(config).unwrap();

    //let job = model::CIJob::new("other-branch", "master", &ctx).unwrap();
    let server = server::run_server(&ctx);
    //job.ensure_running().unwrap();
}
