use futures::future;
use hyper::{Body, Chunk, Method, Request, Response, Server, StatusCode};
use hyper::rt::{Future, Stream};
use serde_json::Value;
use failure::Error;

use crate::model::CIJob;

type BoxFut = Box<Future<Item=Response<Body>, Error=Error> + Send>;


// struct WebHookListener {
//     config: Config,
// }


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
fn process_mr_hook(body: &Body) -> BoxFut {
    let fut = body.concat2()
        .and_then(|body| {
            let body = body.to_vec();
            let hook: Value = serde_json::from_slice(&body)?;
            Ok(hook)
        });
    Box::new(fut)
}



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
struct Service<'a> {
    ci_job: &'a mut CIJob<'a>,
}

impl<'a> hyper::service::Service for Service<'a> {
    type ReqBody = Body;
    type ResBody = Body;
    type Error = Error;
    type Future = BoxFut;

    fn call(&mut self, req: Request<Self::ReqBody>) -> Self::Future {
        let mut response = Response::new(Body::empty());

        if let Some(hv) = req.headers().get("X-Gitlab-Event") {
            if hv == "Merge Request Hook" {
                return process_mr_hook(req.body());
            }
        }

        Box::new(future::ok(response))
    }
}

pub fn run_server(ci_job: &mut CIJob) {
    // This is our socket address...
    let addr = ([127, 0, 0, 1], 3000).into();

    // A `Service` is needed for every connection, so this
    // creates one of our specialized struct
    let new_svc = || {
        Service { ci_job }
    };

    let server = Server::bind(&addr)
        .serve(new_svc)
        .map_err(|e| eprintln!("server error: {}", e));

    // Run this server for... forever!
    hyper::rt::run(server);
}