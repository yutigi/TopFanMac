fn main() {
    let s = smc::Smc::open().unwrap();
    for pass in 0..2 {
        if pass > 0 {
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
        print!("pass {pass}: ");
        for k in [
            "F0Ac", "F0Mn", "F0Mx", "F0Tg", "F0Md", "F1Ac", "F1Tg", "F1Md",
        ] {
            let b: [u8; 4] = k.as_bytes().try_into().unwrap();
            match s.read(smc::Key::new(&b)) {
                Ok(v) => print!("{k}={:?}  ", v),
                Err(e) => print!("{k}=ERR({e})  "),
            }
        }
        println!();
    }
}
