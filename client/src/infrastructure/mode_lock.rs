// This file is part of flashvnc, a VNC client.
// Copyright 2018 Patrick Plagwitz
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

use std::ops::{Deref,DerefMut};
use std::cell::UnsafeCell;
use std::sync::{Condvar,Mutex};

pub struct ModeLockGuard<'a, T : 'a> {
    lock: &'a ModeLock<T>
}
impl<'a, T> ModeLockGuard<'a, T> {
    unsafe fn new(lock : &'a ModeLock<T>) -> Self {
        Self {
            lock: lock
        }
    }
}
impl<'a, T> Deref for ModeLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}
impl<'a, T> DerefMut for ModeLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}
impl<'a, T> Drop for ModeLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.unlock();
    }
}

struct State {
    no_of_clients : usize,
    current_mode : u32
}

pub struct ModeLock<T> {
    data: UnsafeCell<T>,
    state_signal : Condvar,
    state : Mutex<State>
}
impl<T> ModeLock<T> {
    pub fn new(t : T) -> Self {
        Self {
            data: UnsafeCell::new(t),
            state_signal: Condvar::new(),
            state: Mutex::new(State {
                no_of_clients: 0,
                current_mode: 0
            })
        }
    }

    pub fn lock<M : Into<u32>>(&self, mode : M) -> ModeLockGuard<T> {
        let mode : u32 = mode.into();
        let mut state = self.state.lock().unwrap();
        while state.no_of_clients != 0 
            && state.current_mode != mode
        {
            state = self.state_signal.wait(state).unwrap();
        }
        state.no_of_clients += 1;
        state.current_mode = mode;
        unsafe { ModeLockGuard::new(self) }
    }

    fn unlock(&self) {
        let mut state = self.state.lock().unwrap();
        state.no_of_clients -= 1;
        self.state_signal.notify_all();
    }
}

//unsafe impl<T : ?Sized> UnwindSafe for ModeLock<T> { }
//unsafe impl<T : ?Sized> RefUnwindSafe for ModeLock<T> { }
unsafe impl<T : Send + Sync> Send for ModeLock<T> { }
unsafe impl<T : Send + Sync> Sync for ModeLock<T> { }

#[cfg(test)]
mod a_mode_lock {
    use super::*;
    use std;
    use std::time::{Duration,Instant};
    use std::sync::Arc;

    enum TestMode {
        A, B
    }
    impl From<TestMode> for u32 {
        fn from(m : TestMode) -> u32 {
            match m {
                TestMode::A => 0,
                TestMode::B => 1
            }
        }
    }

    #[test]
    fn should_provide_access_to_the_wrapped_object() {
        let lock = ModeLock::new(5);
        let mut n = lock.lock(TestMode::A);
        *n += 2;
        assert_eq!(*n, 7);
    }

    #[test]
    fn should_allow_simultaneous_mutable_access_with_the_same_mode() {
        let lock = ModeLock::new(5);
        {
            let mut n_1 = lock.lock(0u32);
            let mut n_2 = lock.lock(0u32);
            *n_1 += 2;
            *n_2 += 1;
        }
        assert_eq!(*lock.lock(1u32), 8);
    }

    #[test]
    fn should_make_clients_wait_as_long_as_another_mode_is_active() {
        let lock = Arc::new(ModeLock::new(0u32));
        let lock_1 = lock.clone();
        let lock_2 = lock.clone();
        let start = Instant::now();

        let thread_1 = std::thread::spawn(move || {
            let mut n = lock_1.lock(0u32);
            std::thread::sleep(Duration::from_millis(15));
            *n += 1;
        });
        let thread_2 = std::thread::spawn(move || {
            let mut n = lock_2.lock(1u32);
            std::thread::sleep(Duration::from_millis(15));
            *n += 1;
        });
        thread_1.join().unwrap();
        thread_2.join().unwrap();
        let n = lock.lock(0u32);
        assert_eq!(*n, 2);

        assert!(Instant::now().duration_since(start) > 
                Duration::from_millis(30));
    }
}
