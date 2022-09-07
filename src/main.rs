use actix_web;
use actix_web::rt::spawn;
use actix_web::{
    get, middleware::Logger, post, put, web, App, HttpResponse, HttpServer, Responder,
};
use clap;
use clap::Parser;
use env_logger;
use lazy_static::lazy_static;
use log;
use oj;
use oj::{
    compare_users, get_score_list, get_user_submissions, match_job, run_job, Config, Job, PostJob,
    Reason, User, UserRank,
};
use std::borrow::BorrowMut;
use std::cmp::Ordering;
use std::fs;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use actix_web::dev::Path;
lazy_static! {
    static ref JOB_LIST: Arc<Mutex<Vec<Job>>> = Arc::new(Mutex::new(Vec::new()));
}
lazy_static! {
    static ref UESR_LIST: Arc<Mutex<Vec<User>>> = Arc::new(Mutex::new(vec![]));
}
#[get("/hello/{name}")]
async fn greet(name: web::Path<String>) -> impl Responder {
    log::info!(target: "greet_handler", "Greeting {}", *name);
    format!("Hello {name}!")
}

// DO NOT REMOVE: used in automatic testing
#[post("/internal/exit")]
#[allow(unreachable_code)]
async fn exit() -> impl Responder {
    save_data();
    log::info!("Shutdown as requested");
    std::process::exit(0);
    format!("Exited")
}

#[post("/jobs")]
async fn post_jobs(body: web::Json<PostJob>, config: web::Data<Config>) -> impl Responder {
    let response: HttpResponse;
    let mut lock = JOB_LIST.lock().unwrap();

    let id = lock.len();
    //create a job
    let mut job = Job::new(id as i32, &body.0);
    if job.submission.user_id >= UESR_LIST.lock().unwrap().len() as i32 {
        return HttpResponse::NotFound().json(oj::Error {
            reason: Reason::ErrNotFound,
            code: 3,
            message: "".to_string(),
        });
    }
    match run_job(&mut job, &config) {
        Ok(_) => response = HttpResponse::Ok().json(&job),
        Err(err) => response = HttpResponse::NotFound().json(err),
    };
    //push modified job
    lock.push(job.clone());
    HttpResponse::Ok().json(job)
}

#[get("/jobs")]
async fn get_jobs(body: web::Query<oj::GetJob>) -> impl Responder {
    let mut return_list: Vec<Job> = vec![];
    for i in &*JOB_LIST.lock().unwrap() {
        if match_job(&body, i, UESR_LIST.lock().unwrap().as_ref()) {
            return_list.push(i.clone());
        }
    }
    println!("1");
    return HttpResponse::Ok().json(return_list);
}

#[get("/jobs/{job_id}")]
async fn get_job(job_id: web::Path<i32>) -> impl Responder {
    let mut job: Option<Job> = None;
    let id: i32 = job_id.into_inner();
    for i in &*JOB_LIST.lock().unwrap() {
        if i.id == id {
            job = Some(i.clone());
        }
    }
    return match job {
        Some(a) => HttpResponse::Ok().json(a),
        None => HttpResponse::NotFound()
            .json("{ reason=ERR_NOT_FOUND, code=3, message=\"Job 123456 not found.\"}"),
    };
}

#[put("/jobs/{job_id}")]
async fn put_job(job_id: web::Path<i32>, config: web::Data<Config>) -> impl Responder {
    let mut job: Option<&mut Job> = None;
    let id: i32 = job_id.into_inner();
    let mut lock = JOB_LIST.lock().unwrap();
    for i in lock.borrow_mut().iter_mut() {
        if i.id == id {
            job = Some(i);
        }
    }
    if let None = job {
        return HttpResponse::NotFound().json(oj::Error {
            reason: Reason::ErrNotFound,
            code: 3,
            message: "Job 123456 not found.".to_string(),
        });
    }
    let mut job = job.unwrap();
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

#[get("/users")]
async fn get_users() -> impl Responder {
    HttpResponse::Ok().json(UESR_LIST.lock().unwrap().to_vec())
}

#[post("/users")]
async fn post_users(user: web::Json<User>) -> impl Responder {
    let mut user = user.deref().clone();
    let mut is_name_in = false;
    let mut is_id_in = false;
    for i in UESR_LIST.lock().unwrap().iter_mut() {
        if user.id == i.id {
            i.name = user.name.clone();
            is_id_in = true;
            break;
        }
        if user.name == i.name {
            is_name_in = true;
        }
    }
    //name in, error directly
    return if is_name_in {
        HttpResponse::BadRequest().json(oj::Error {
            reason: Reason::ErrInvalidArgument,
            code: 1,
            message: format!("User name '{}' already exists.", user.name),
        })
    } else if user.id.is_none() {
        user.id = Some(UESR_LIST.lock().unwrap().len() as i32);
        UESR_LIST.lock().unwrap().push(user.clone());
        HttpResponse::Ok().json(user)
    } else if !is_id_in {
        HttpResponse::NotFound().json(oj::Error {
            reason: Reason::ErrNotFound,
            code: 3,
            message: format!("User {} already exists.", user.id.unwrap()),
        })
    } else {
        HttpResponse::Ok().json(user)
    };
}

#[get("/contests/{contest_id}/ranklist")]
async fn get_rank_list(
    contest_id: web::Path<i32>,
    rule: web::Query<oj::RankRule>,
    config: web::Data<Config>,
) -> impl Responder {
    let mut user_list: Vec<User> = UESR_LIST.lock().unwrap().deref().clone();
    let rule = rule.deref();
    let job_list = JOB_LIST.lock().unwrap().deref().to_vec();
    user_list.sort_by(|a, b| {
        let a_list = get_user_submissions(a, &job_list);
        let b_list = get_user_submissions(b, &job_list);
        let a_tuple = get_score_list(&job_list,&a_list, rule, config.deref());
        let a_score: f64 = a_tuple.0.iter().sum();
        let b_tuple = get_score_list(&job_list,&b_list, rule, config.deref());
        let b_score: f64 = b_tuple.0.iter().sum();
        let order = compare_users(
            &b_list,
            &a_list,
            (b_score, a_score),
            (b_tuple.1, a_tuple.1),
            rule,
        );
        if let Ordering::Equal = order {
            a.id.cmp(&b.id)
        } else {
            order
        }
    });
    let mut rank: Vec<UserRank> = vec![];

    let list = get_user_submissions(&user_list[0], &job_list);
    let scores = get_score_list(&job_list,&list, rule, config.deref()).0;
    rank.push(UserRank {
        user: user_list[0].clone(),
        rank: 1,
        scores,
    });
    for i in 1..user_list.len() {
        let former_list = get_user_submissions(&user_list[i - 1], &job_list);
        let now_list = get_user_submissions(&user_list[i], &job_list);
        let f = get_score_list(&job_list,&former_list, rule, config.deref());
        let f_score: f64 = f.0.iter().sum();
        let f_index = f.1;
        let n = get_score_list(&job_list,&now_list, rule, config.deref());
        let n_score: f64 = n.0.iter().sum();
        let n_index = n.1;
        let scores = get_score_list(&job_list,&now_list, rule, config.deref()).0;

        if Ordering::Equal
            == compare_users(
            &former_list,
            &now_list,
            (f_score, n_score),
            (f_index, n_index),
            rule,
        )
        {
            if !former_list.is_empty() && !now_list.is_empty() {
                println!(
                    "{},{}",
                    former_list[0].submission.user_id, now_list[0].submission.user_id
                );
            }
            rank.push(UserRank {
                user: user_list[i].clone(),
                rank: rank[i - 1].rank,
                scores,
            });
        } else {
            rank.push(UserRank {
                user: user_list[i].clone(),
                rank: (i + 1) as i32,
                scores,
            });
        }
    }
    HttpResponse::Ok().json(rank)
}

// #[get("/contest")]
// async fn get_contest()->impl Responder{
//
// }

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = oj::Args::parse();
    let path = args.config;
    let is_flush = args.flush_data;
    let config: Config = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    if is_flush {
        UESR_LIST.lock().unwrap().push(User {
            id: Some(0),
            name: "root".to_string(),
        });
    } else {
        let mut job_lock = JOB_LIST.lock().unwrap();
        let jobs_string = fs::read_to_string("./jobs.json").unwrap();
        let initial_jobs: Vec<Job> = serde_json::from_str(&jobs_string).unwrap();
        *job_lock = initial_jobs;
        let mut user_lock = UESR_LIST.lock().unwrap();
        let user_string = fs::read_to_string("./users.json").unwrap();
        let initial_users: Vec<User> = serde_json::from_str(&user_string).unwrap();
        *user_lock = initial_users;
    }
    spawn(async {
        let mut interval = actix_web::rt::time::interval(std::time::Duration::from_micros(500000));
        loop {
            interval.tick().await;
            save_data();
        }
    });
    // save_task.
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
            .service(post_users)
            .service(get_users)
            .service(get_rank_list)
            // DO NOT REMOVE: used in automatic testing
            .service(exit)
    })
        .bind(("127.0.0.1", 12345))?
        .run()
        .await
}

fn save_data() {
    let jobs_lock = JOB_LIST.lock().unwrap();
    let jobs: String = serde_json::to_string_pretty(&*jobs_lock).unwrap();
    fs::write("./jobs.json", jobs).unwrap();
    let users_lock = UESR_LIST.lock().unwrap();
    let users: String = serde_json::to_string_pretty(&*users_lock).unwrap();
    fs::write("./users.json", users).unwrap();
}
