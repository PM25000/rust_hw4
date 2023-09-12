use std::{
    alloc::System,
    cell::RefCell,
    future::{self, Future},
    sync::{Arc, Condvar, Mutex},
    task::{Context, Poll, RawWaker, RawWakerVTable, Wake, Waker},
};

use futures::future::BoxFuture;

use scoped_tls::scoped_thread_local;
use std::collections::VecDeque;

// struct Demo;

// impl Future for Demo {
//     type Output = ();

//     fn poll(
//         self: std::pin::Pin<&mut Self>,
//         _cx: &mut std::task::Context<'_>,
//     ) -> std::task::Poll<Self::Output> {
//         println!("Hello, world????");
//         std::task::Poll::Ready(())
//     }
// }

fn dummy_waker() -> Waker {
    static DATA: () = ();
    unsafe { Waker::from_raw(RawWaker::new(&DATA, &VTABLE)) }
}

const VTABLE: RawWakerVTable =
    RawWakerVTable::new(vtable_clone, vtable_wake, vtable_wake_by_ref, vtable_drop);

unsafe fn vtable_clone(ptr: *const ()) -> RawWaker {
    println!("vtable_clone");
    RawWaker::new(ptr, &VTABLE)
}

unsafe fn vtable_wake(_ptr: *const ()) {
    println!("vtable_wake");
}

unsafe fn vtable_wake_by_ref(_ptr: *const ()) {
    println!("vtable_wake_by_ref");
}

unsafe fn vtable_drop(_ptr: *const ()) {
    println!("vtable_drop");
}


struct Signal {
    state: Mutex<State>,
    cond: Condvar,
}

enum State {
    Empty,
    Wating,
    Notified,
}

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.notify();
    }
}

impl Signal {
    fn new() -> Self {
        Self {
            state: Mutex::new(State::Empty),
            cond: Condvar::new(),
        }
    }
    fn wait(&self) {
        let mut state = self.state.lock().unwrap();
        match *state {
            State::Empty => {
                *state = State::Wating;
                while let State::Wating = *state {
                    state = self.cond.wait(state).unwrap();
                }
            }
            State::Wating => {
                panic!("cannot wait twice");
            }
            State::Notified => {
                *state = State::Empty;
            }
        }
    }
    fn notify(&self) {
        let mut state = self.state.lock().unwrap();
        match *state {
            State::Empty => {
                *state = State::Notified;
            }
            State::Wating => {
                *state = State::Empty;
                self.cond.notify_one();
            }
            State::Notified => {
                println!("already notified")
            }
        }
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut fut = std::pin::pin!(future);
    let signal = Arc::new(Signal::new());
    let waker = Waker::from(signal.clone());

    let mut cx = Context::from_waker(&waker);
    // loop {
    //     if let Poll::Ready(val) = fut.as_mut().poll(&mut cx) {
    //         return val;
    //     }
    // }

    let runnable = Mutex::new(VecDeque::with_capacity(1024));
    SIGNAL.set(&signal, || {
        RUNNABLE.set(&runnable, || loop {
            if let Poll::Ready(output) = fut.as_mut().poll(&mut cx) {
                return output;
            }
            while let Some(task) = runnable.lock().unwrap().pop_front() {
                let waker = Waker::from(task.clone());
                let mut cx = Context::from_waker(&waker);
                let _ = task.future.borrow_mut().as_mut().poll(&mut cx);
            }
            signal.wait();
        })
    })
}

fn spawn<F: Future<Output = ()> + 'static + Send>(future: F) {
    let signal = Arc::new(Signal::new());
    let waker = Waker::from(signal.clone());
    let task = Arc::new(Task {
        future: RefCell::new(Box::pin(future)),
        signal: signal.clone(),
    });
    let mut cx = Context::from_waker(&waker);
    if let Poll::Ready(_) = task.future.borrow_mut().as_mut().poll(&mut cx) {
        return;
    }
    RUNNABLE.with(|runnable| {
        runnable.lock().unwrap().push_back(task);
        signal.notify();
    })
}

async fn demo() {
    let (tx, rx) = async_channel::bounded::<()>(1);
    std::thread::spawn(move || {
        // std::thread::sleep(std::time::Duration::from_secs(2));
        tx.send_blocking(()).unwrap();
    });
    let _ = rx.recv().await;
    println!("Hello, world!111");
    //sleep
}

async fn demo1() {
    let (tx, rx) = async_channel::bounded::<()>(1);
    println!("Hello, world!222");
    spawn(demo2(rx));
    println!("Hello, world!444");
    tx.send(()).await.unwrap();
}

async fn demo2(rx: async_channel::Receiver<()>) {
    println!("Hello, world!333");
    rx.recv().await.unwrap();
    println!("Hello, world!333");

    // tx.send(()).await.unwrap();
}

struct Task {
    future: RefCell<BoxFuture<'static, ()>>,
    signal: Arc<Signal>,
}
unsafe impl Send for Task {}
unsafe impl Sync for Task {}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        RUNNABLE.with(|runnable| {
            runnable.lock().unwrap().push_back(self.clone());
            self.signal.notify();
        })
    }
}

scoped_thread_local!(static RUNNABLE: Mutex<VecDeque<Arc<Task>>>);
scoped_thread_local!(static SIGNAL: Arc<Signal>);

fn main() {
    println!("Hello, world!");

    block_on(demo());
    block_on(demo());
    block_on(demo());

    block_on(demo1());
}
