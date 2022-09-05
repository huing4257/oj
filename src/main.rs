use std::borrow::{Borrow, BorrowMut};
use actix_web::{get, put, middleware::Logger, post, web, App, HttpResponse, HttpServer, Responder};
use clap::Parser;
use env_logger;
use log;
use oj;
use oj::{Config, PostJob, run_job, Job, match_job, Reason};
use std::fs;
use actix_web;
use std::sync::{Arc, Mutex};
use actix_web::web::Path;
use lazy_static::lazy_static;
lazy_static! {
    static ref JOB_LIST: Arc<Mutex<Vec<Job >>> = Arc::new(Mutex::new(Vec::new()));
}
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
    println!("0");
    let response:HttpResponse;
    let mut lock =JOB_LIST.lock().unwrap();
    let id=lock.len();
    //create a job
    let mut job=Job::new(id as i32,&body.0);
    match run_job(&mut job,&config) {
        Ok(_) => {
            response=HttpResponse::Ok().json(&job)
        }
        Err(err) => {
            response= HttpResponse::NotFound().json(err)
        }
    };
    //push modified job
    lock.push(job);
    response
}


#[get("/jobs")]
async fn get_jobs(body: web::Query<oj::GetJob>) -> impl Responder {
    let mut return_list: Vec<Job> = vec![];
    for i in &*JOB_LIST.lock().unwrap() {
        if match_job(&body, i) {
            return_list.push(i.clone());
        }
    }
    println!("1");
    return HttpResponse::Ok().json(return_list);
}

#[get("/jobs/{job_id}")]
async fn get_job(job_id: Path<i32>) -> impl Responder {
    let mut job: Option<Job> = None;
    let id: i32 = job_id.into_inner();
    for i in &*JOB_LIST.lock().unwrap() {
        if i.id == id {
            job = Some(i.clone());
        }
    }
    return match job {
        Some(a) => {
            HttpResponse::Ok().json(a)
        }
        None => {
            HttpResponse::NotFound().json("{ reason=ERR_NOT_FOUND, code=3, message=\"Job 123456 not found.\"}")
        }
    };
}

#[put("/jobs/{job_id}")]
async fn put_job(job_id: Path<i32>, config: web::Data<Config>) -> impl Responder {
    let mut job: Option<&mut Job> = None;
    let id: i32 = job_id.into_inner();
    let mut lock=JOB_LIST.lock().unwrap();
    for i in lock.borrow_mut().iter_mut(){
        if i.id == id {
            job = Some(i);
        }
    }
    if let None=job{
        return HttpResponse::NotFound().json(oj::Error{
            reason: Reason::ErrNotFound,
            code: 3,
            message: "Job 123456 not found.".to_string()
        })
    }
    let mut job=job.unwrap();
    match run_job(&mut job, &config) {
        Ok(job_response) => {
            HttpResponse::Ok().json(job_response);
        }
        Err(err) => {
            HttpResponse::NotFound().json(err);
        }
    }
    HttpResponse::Ok().json(job.clone())
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
            .service(get_jobs)
            .service(get_job)
            .service(put_job)
            // DO NOT REMOVE: used in automatic testing
            .service(exit)
    })
        .bind(("127.0.0.1", 12345))?
        .run()
        .await
}
