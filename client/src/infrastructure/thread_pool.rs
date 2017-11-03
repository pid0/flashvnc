use std::thread::JoinHandle;
use std::thread::Builder as ThreadBuilder;
use std::sync::mpsc;
use std::sync::{Arc,Mutex};
use std::panic::{catch_unwind,AssertUnwindSafe};
use std::marker::PhantomData;
use std::cell::RefCell;

pub trait Value : Send + 'static { }
impl<T : Send + 'static> Value for T { }

type JobReceiver<S> = Arc<Mutex<mpsc::Receiver<Box<Job<S>>>>>;
pub type JobResult<E> = Result<(), Error<E>>;
pub type JobFuncResult<E> = Result<(), E>;

#[derive(Debug)]
pub enum Error<E> {
    Panic,
    Value(E)
}

#[must_use]
pub struct Future<E : Value> {
    receiver : mpsc::Receiver<JobResult<E>>,
    has_waited : RefCell<bool>
}
impl<E : Value> Future<E> {
    pub fn new(result_receiver : mpsc::Receiver<JobResult<E>>) -> Self {
        Self {
            receiver: result_receiver,
            has_waited: RefCell::new(false)
        }
    }
    pub fn wait(&self) -> JobResult<E> {
        *self.has_waited.borrow_mut() = true;
        self.receiver.recv().unwrap()
    }
}
impl<E : Value> Drop for Future<E> {
    fn drop(&mut self) {
        if !*self.has_waited.borrow() {
            self.wait().unwrap_or(());
        }
    }
}

#[must_use]
pub struct FutureCollection<E : Value> {
    futures : Vec<Future<E>>
}
impl<E : Value> FutureCollection<E> {
    pub fn new(futures : Vec<Future<E>>) -> Self {
        Self {
            futures: futures
        }
    }

    pub fn wait(mut self) -> Result<(), Vec<Error<E>>> {
        let mut errors = Vec::new();
        for future in self.futures.drain(..) {
            if let Err(err) = future.wait() {
                errors.push(err);
            }
        }
        if errors.len() == 0 {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

struct AssertSend<T> {
    t : PhantomData<T>
}
impl<T> AssertSend<T> {
    fn new() -> Self {
        Self {
            t: PhantomData
        }
    }
}
unsafe impl<T> Send for AssertSend<T> { }

trait Job<S : 'static> : Send {
    fn execute(self : Box<Self>, state : &mut S);
}
struct RespondingJob<E : Value, S : 'static, F>
    where F : FnOnce(&mut S) -> JobFuncResult<E> + Value
{
    f : F,
    response_sender : mpsc::Sender<JobResult<E>>,
    _s : AssertSend<S>
}
impl<E : Value, S : 'static, F> RespondingJob<E, S, F>
    where F : FnOnce(&mut S) -> JobFuncResult<E> + Value
{
    fn new(f : F, response_sender : mpsc::Sender<JobResult<E>>) -> Self
    {
        Self {
            f: f,
            response_sender: response_sender,
            _s: AssertSend::new()
        }
    }
}
impl<E : Value, S : 'static, F> Job<S> for RespondingJob<E, S, F>
    where F : FnOnce(&mut S) -> JobFuncResult<E> + Value
{
    fn execute(self : Box<Self>, state : &mut S) {
        let response_sender = self.response_sender.clone();
        let func = self.f;
        let result = match catch_unwind(AssertUnwindSafe(|| func(state))) {
            Ok(result) => match result {
                Ok(()) => Ok(()),
                Err(e) => Err(Error::Value(e))
            },
            Err(_) => Err(Error::Panic)
        };
        response_sender.send(result).unwrap();
    }
}

fn pop_job<S : 'static>(receiver : &JobReceiver<S>) 
    -> Result<Box<Job<S>>, mpsc::RecvError>
{
    receiver.lock().unwrap().recv()
}
fn worker_main<S : 'static>(
    receiver : JobReceiver<S>,
    mut state : S) 
{
    while let Ok(job) = pop_job(&receiver) {
        job.execute(&mut state);
    }
}

pub struct ThreadPool<S : 'static> {
    threads : Vec<JoinHandle<()>>,
    job_sender : Option<mpsc::Sender<Box<Job<S>>>>,
    s : PhantomData<S>
}
impl<S : 'static> ThreadPool<S> {
    pub fn new<Init>(name_prefix : &str, size : usize, 
                     init_state : Init) -> Self
        where Init : Fn() -> S + Value
    {
        let (job_sender, job_receiver) = mpsc::channel();
        let job_receiver = Arc::new(Mutex::new(job_receiver));
        let init_state = Arc::new(Mutex::new(init_state));

        let mut threads = Vec::new();
        for i in 0..size {
            let job_receiver = job_receiver.clone();
            let init_state = init_state.clone();
            let thread = ThreadBuilder::new()
                .name(format!("{}-{}", name_prefix, i))
                .spawn(move || {
                    let state = {
                        (init_state.lock().unwrap())()
                    };
                    drop(init_state);
                    worker_main(job_receiver, state); 
                }).unwrap();
            threads.push(thread);
        }

        Self {
            threads: threads,
            job_sender: Some(job_sender),
            s: PhantomData
        }
    }

    pub fn spawn_fn<F, E : Value>(&self, f : F) -> Future<E>
        where F : FnOnce(&mut S) -> JobFuncResult<E> + Value
    {
        let (sender, receiver) = mpsc::channel();
        self.job_sender.as_ref().unwrap().send(
            Box::new(RespondingJob::new(f, sender))).unwrap();
        Future::new(receiver)
    }
}
impl<S : 'static> Drop for ThreadPool<S> {
    fn drop(&mut self) {
        self.job_sender = None;
        for thread in self.threads.drain(..) {
            thread.join().unwrap();
        }
    }
}

#[cfg(test)]
mod the_thread_pool {
    use super::*;
    use std::time::{Instant,Duration};
    use std::thread::sleep;
    use std::sync::{Arc,Mutex};

    #[derive(Debug)]
    struct TestError(u32);

    fn fixture_with_state<Init, State>(init : Init) -> ThreadPool<State>
        where Init : Fn() -> State + Value
    {
        ThreadPool::new("test", 4, init)
    }
    fn fixture() -> ThreadPool<()> {
        ThreadPool::new("test", 4, || { })
    }

    #[test]
    fn should_run_closures_in_parallel() {
        let n = Arc::new(Mutex::new(0));
        let n_1 = n.clone();
        let n_2 = n.clone();

        let pool = fixture();
        let start = Instant::now();
        let f_1 : Future<()> = pool.spawn_fn(move |_| {
            sleep(Duration::from_millis(110));
            *n_1.lock().unwrap() += 1;
            Ok(())
        });
        let f_2 : Future<()> = pool.spawn_fn(move |_| {
            sleep(Duration::from_millis(110));
            *n_2.lock().unwrap() += 1;
            Ok(())
        });

        while *n.lock().unwrap() != 2 {
            assert!(Instant::now().duration_since(start) <
                    Duration::from_millis(200));
        }
        assert!(Instant::now().duration_since(start) <
                Duration::from_millis(200));
        f_1.wait().unwrap(); f_2.wait().unwrap();
    }

    #[test]
    fn should_return_a_future_for_a_job() {
        let pool = fixture();

        let future = pool.spawn_fn(|_| {
            Err(TestError(5))
        });
        match future.wait() {
            Err(Error::Value(e)) => assert_eq!(e.0, 5),
            _ => panic!("must be error")
        }
    }

    #[test]
    fn should_catch_panics_and_return_them_as_errors() {
        let pool = fixture();

        let future : Future<()> = pool.spawn_fn(|_| {
            panic!();
        });

        match future.wait() {
            Err(Error::Panic) => { },
            _ => {
                panic!("must be panic");
            }
        }
    }

    #[test]
    fn should_have_a_method_to_provide_state_to_a_thread() {
        let pool = fixture_with_state(|| 5);
        let n = Arc::new(Mutex::new(0));
        let n_clone = n.clone();

        let f : Future<()> = pool.spawn_fn(move |x| {
            *x += 1;
            *n_clone.lock().unwrap() = *x;
            Ok(())
        });
        f.wait().unwrap();

        assert_eq!(*n.lock().unwrap(), 6);
    }
}
