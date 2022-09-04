use actix_web::{get, middleware::Logger, post, web, App, HttpResponse, HttpServer, Responder};
use chrono::prelude::*;
use clap::Parser;
use env_logger;
use log;
use oj;
use oj::{Config, Language, PostJob, Problem, run_job, Reason, get_language_problem};
use std::borrow::BorrowMut;
use std::fs;
use std::fs::create_dir;
use std::process::{Command, Stdio};
use actix_web::body::None;
use actix_web::web::{Data, Json};
use serde::Serialize;

#[get("/hello/{name}")]
async fn greet(name: web::Path<String>) -> impl Responder {
    log::info!(target: "greet_handler", "Greeting {}", name);
    format!("Hello {name}!")
}

// DO NOT REMOVE: used in automatic testing
#[post("/internal/exit")]
#[allow(unreachable_code)]
async fn exit() -> impl Responder {
    log::info!("Shutdown as requested");
    std::process::exit(0);
    format!("Exited")
}

#[post("/jobs")]
async fn post_jobs(body: web::Json<PostJob>, config: web::Data<Config>) -> impl Responder {
    let mut http_response=HttpResponse::new(Default::default());
    let (current_language, current_problem) = get_language_problem(&body, &config);
    if current_language.is_none()||current_problem.is_none() {
        return  HttpResponse::BadRequest().json({});
    }
    let mut current_language = current_language.unwrap();
    let mut current_problem = current_problem.unwrap();

    match  run_job(&mut current_language,&mut current_problem,&body){
        Ok(job_response)=>{
            return HttpResponse::Ok().json(job_response)
        }
        Err(err)=>{
            match err {
                Reason::ErrNotFound=> {
                    return  HttpResponse::BadRequest().json("{}")
                }
                _=>unimplemented!()
            }

        }
    }
}



#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let path = oj::Args::parse().config;
    let config: Config = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(config.clone()))
            .route("/hello", web::get().to(|| async { "Hello World!" }))
            .service(greet)
            .service(post_jobs)
            // DO NOT REMOVE: used in automatic testing
            .service(exit)
    })
    .bind(("127.0.0.1", 12345))?
    .run()
    .await
}
