use objc::{class, msg_send, sel, sel_impl};

fn main() {
    unsafe {
        let nsevent_class = class!(NSEvent);
        let _: (f64, f64) = msg_send![nsevent_class, mouseLocation];
        println!("Successfully called mouseLocation");
    }
}
