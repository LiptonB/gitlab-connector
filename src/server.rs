use futures::future;
use hyper::{Body, Request, Response, Server};
use hyper::service::service_fn;
use hyper::rt::{Future, Stream};
use serde_json::Value;
use std::sync::Arc;

use crate::model::{Context, CIJob};

type BoxFut = Box<Future<Item=Response<Body>, Error=hyper::Error> + Send>;
type Result<T> = std::result::Result<T, failure::Error>;


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

fn execute_mr_hook(body: &[u8], ctx: &Context) -> Result<()> {
    let hook: Value = serde_json::from_slice(body)?;
    if let (Some(source), Some(target)) = (
        hook["object_attributes"]["source_branch"].as_str(),
        hook["object_attributes"]["target_branch"].as_str()
    ) {
        let job = CIJob::new(source, target, ctx)?;
        job.ensure_running()?;
    }
    Ok(())
}

/*
fn execute_pipeline_hook(body: &[u8], ctx: Arc<Context>) -> Result<()> {
    let branch = "";
    let base = "";
    // TODO: figure out whether for a base branch or a MR
    let job = CIJob::new(branch, base, &ctx)?;
    job.update_statuses();
    Ok(())
}
*/
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


// TODO: log instead of returning error, no one will see it
fn process_mr_hook(body: Body, ctx: Arc<Context>) -> BoxFut {
    let mut response = Response::new(Body::empty());

    let fut = body
        .concat2()
        .map(move |body| {
            let body = body.to_vec();
            match execute_mr_hook(&body, &ctx) {
                Ok(_) => {
                    *response.body_mut() = Body::from("Success");
                }
                Err(e) => {
                    *response.body_mut() = Body::from(e.to_string());
                }
            }
            response
        });
    Box::new(fut)
}

fn process_request(req: Request<Body>, ctx: Arc<Context>) -> BoxFut {
    if let Some(hv) = req.headers().get("X-Gitlab-Event") {
        if hv == "Merge Request Hook" {
            return process_mr_hook(req.into_body(), ctx);
        }
    }

    Box::new(future::ok(Response::new(Body::empty())))
}

pub fn run_server(ctx: Context) {
    // This is our socket address...
    let addr = ([127, 0, 0, 1], 3000).into();

    let ctx = Arc::new(ctx);
    // A `Service` is needed for every connection, so this
    // creates one of our specialized struct
    let new_svc = move || {
        let ctx = ctx.clone();
        service_fn(move |req: Request<Body>| process_request(req, ctx.clone()))
    };

    let server = Server::bind(&addr)
        .serve(new_svc)
        .map_err(|e| eprintln!("server error: {}", e));

    // Run this server for... forever!
    hyper::rt::run(server);
}
