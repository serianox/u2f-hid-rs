use std::collections::HashSet;
use std::io;
use std::iter::FromIterator;
use std::sync::mpsc::{channel, Receiver, TryIter};
use std::thread;

use super::iohid::*;
use super::iokit::*;
use core_foundation_sys::base::*;
use core_foundation_sys::runloop::*;
use runloop::RunLoop;
use util::to_io_err;

extern crate log;
extern crate libc;
use libc::c_void;

pub enum Event {
    Add(IOHIDDeviceID),
    Remove(IOHIDDeviceID),
}

pub struct Monitor {
    // Receive events from the thread.
    rx: Receiver<Event>,
    // Handle to the thread loop.
    thread: RunLoop,
}

impl Monitor {
    pub fn new() -> io::Result<Self> {
        let (tx, rx) = channel();

        let thread = RunLoop::new(
            move |alive| -> io::Result<()> {
                // Create and initialize a scoped HID manager.
                let manager = IOHIDManager::new()?;

                // Match only U2F devices.
                let dict = IOHIDDeviceMatcher::new();
                unsafe { IOHIDManagerSetDeviceMatching(manager.get(), dict.get()) };

                let mut stored = HashSet::new();

                // Run the Event Loop. CFRunLoopRunInMode() will dispatch HID
                // input reports into the various callbacks
                while alive() {
                    trace!("OSX Runloop running, handle={:?}", thread::current());

                    // TODO
                    let device_set = unsafe { IOHIDManagerCopyDevices(manager.get()) };
                    if !device_set.is_null() {
                        let num_devices = unsafe { CFSetGetCount(device_set) };
                        let mut devices: Vec<IOHIDDeviceRef> =
                            Vec::with_capacity(num_devices as usize);
                        unsafe {
                            CFSetGetValues(device_set, devices.as_mut_ptr() as *mut *const c_void);
                        }
                        unsafe {
                            devices.set_len(num_devices as usize);
                        }
                        unsafe { CFRelease(device_set as *mut libc::c_void) };

                        // TODO
                        let devices = HashSet::from_iter(devices);

                        // Remove devices that are gone.
                        for id in stored.difference(&devices) {
                            tx.send(Event::Remove(IOHIDDeviceID::from_ref(*id)))
                                .map_err(to_io_err)?;
                        }

                        // Add devices that were plugged in.
                        for id in devices.difference(&stored) {
                            tx.send(Event::Add(IOHIDDeviceID::from_ref(*id))).map_err(
                                to_io_err,
                            )?;
                        }

                        // Remember the new set.
                        stored = devices;
                    }

                    // TODO read some data ....
                    if unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 0) } ==
                        kCFRunLoopRunStopped
                    {
                        debug!("OSX Runloop device stopped.");
                        break;
                    }
                }
                debug!("OSX Runloop completed, handle={:?}", thread::current());

                Ok(())
            },
            0, /* no timeout */
        )?;

        Ok(Self {
            rx: rx,
            thread: thread,
        })
    }

    pub fn events<'a>(&'a self) -> TryIter<'a, Event> {
        self.rx.try_iter()
    }

    pub fn alive(&self) -> bool {
        self.thread.alive()
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        debug!("OSX Runloop dropped");
        self.thread.cancel();
    }
}
