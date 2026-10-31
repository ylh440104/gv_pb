use std::collections::BTreeMap;

type Word = u32;

const K: [Word; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [Word; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

struct Sha256 {
    state: [Word; 8],
    block: [u8; 64],
    used: usize,
    total: u64,
}

impl Sha256 {
    fn new() -> Self {
        Sha256 {
            state: H0,
            block: [0; 64],
            used: 0,
            total: 0,
        }
    }

    fn round(&mut self) {
        let mut w = [0 as Word; 64];
        for i in 0..16 {
            let mut cell = [0u8; 4];
            cell.copy_from_slice(&self.block[i * 4..i * 4 + 4]);
            w[i] = u32::from_be_bytes(cell);
        }
        for i in 16..64 {
            let first = w[i - 15];
            let second = w[i - 2];
            let s0 = first.rotate_right(7) ^ first.rotate_right(18) ^ (first >> 3);
            let s1 = second.rotate_right(17) ^ second.rotate_right(19) ^ (second >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut v = self.state;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);

            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }

        for i in 0..8 {
            self.state[i] = self.state[i].wrapping_add(v[i]);
        }
    }

    fn feed(&mut self, mut data: &[u8]) {
        self.total += data.len() as u64;
        while !data.is_empty() {
            let room = 64 - self.used;
            let take = if data.len() < room { data.len() } else { room };
            self.block[self.used..self.used + take].copy_from_slice(&data[..take]);
            self.used += take;
            data = &data[take..];
            if self.used == 64 {
                self.round();
                self.used = 0;
            }
        }
    }

    fn digest(mut self) -> [u8; 32] {
        let bits = self.total.wrapping_mul(8);
        self.feed(&[0x80]);
        while self.used != 56 {
            self.feed(&[0]);
        }
        self.total -= 64;
        self.feed(&bits.to_be_bytes());
        let mut out = [0u8; 32];
        for i in 0..8 {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.state[i].to_be_bytes());
        }
        out
    }
}

fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut pad = [0u8; 64];
    if key.len() > 64 {
        let mut hasher = Sha256::new();
        hasher.feed(key);
        pad[..32].copy_from_slice(&hasher.digest());
    } else {
        pad[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0u8; 64];
    let mut opad = [0u8; 64];
    for i in 0..64 {
        ipad[i] = pad[i] ^ 0x36;
        opad[i] = pad[i] ^ 0x5c;
    }

    let mut inner = Sha256::new();
    inner.feed(&ipad);
    inner.feed(message);

    let mut outer = Sha256::new();
    outer.feed(&opad);
    outer.feed(&inner.digest());
    outer.digest()
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{:02x}", byte));
    }
    text
}

fn base_params(ts: u64) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert(String::from("x-param-plat"), String::from("1"));
    map.insert(String::from("x-param-vn"), String::from("4.45.0.4318"));
    map.insert(String::from("x-param-vc"), String::from("44504318"));
    map.insert(String::from("x-param-pkgn"), String::from("com.netease.uuremote"));
    map.insert(String::from("x-param-chn"), String::from("nochannel"));
    map.insert(String::from("x-param-lang"), String::from("zh_CN"));
    map.insert(String::from("x-param-cnt"), String::from("CN"));
    map.insert(String::from("x-param-rel"), String::from("1"));
    map.insert(String::from("x-param-ts"), ts.to_string());
    map
}

fn payload(method: &str, path: &str, params: &BTreeMap<String, String>, body: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (name, value) in params.iter() {
        let lowered = name.to_ascii_lowercase();
        if lowered.starts_with("x-param-") && lowered != "x-param-sign" && !value.is_empty() {
            parts.push(format!("{}={}", lowered, value));
        }
    }
    format!(
        "{}{}{}{}",
        method.to_ascii_uppercase(),
        path,
        parts.join("&"),
        body
    )
}

fn main() {
    let mut pass = 0u32;
    let mut fail = 0u32;

    let key = b"alWiSzXZTLu3WfFnw13uBru3";

    let want_payload = concat!(
        "POST/api/v1/login/check_uu_token",
        "x-param-chn=nochannel&x-param-cnt=CN&x-param-lang=zh_CN",
        "&x-param-pkgn=com.netease.uuremote&x-param-plat=1&x-param-rel=1",
        "&x-param-ts=1700000000&x-param-vc=44504318&x-param-vn=4.45.0.4318{}"
    );
    let got_payload = payload(
        "POST",
        "/api/v1/login/check_uu_token",
        &base_params(1700000000),
        "{}",
    );
    print!("payload matches analysis/sign_vectors.json  ");
    if got_payload == want_payload {
        println!("ok");
        pass += 1;
    } else {
        println!("FAIL");
        fail += 1;
    }

    let want_sign = "489630636a59f4ebf53b720b87abd5d6b6cfd4435809b88d14a27bc04b7c6709";
    let got_sign = hex(&hmac(key, got_payload.as_bytes()));
    print!("signature matches analysis/sign_vectors.json  ");
    if got_sign == want_sign {
        println!("ok");
        pass += 1;
    } else {
        println!("FAIL");
        fail += 1;
    }

    let mut sample = BTreeMap::new();
    sample.insert(String::from("x-param-vn"), String::from("2"));
    sample.insert(String::from("x-param-cnt"), String::from("1"));
    print!("params sorted by key  ");
    if payload("post", "/api/v1/x", &sample, "BODY") == "POST/api/v1/xx-param-cnt=1&x-param-vn=2BODY" {
        println!("ok");
        pass += 1;
    } else {
        println!("FAIL");
        fail += 1;
    }

    let before = payload("GET", "/p", &sample, "");
    sample.insert(String::from("x-param-sign"), String::from("zz"));
    print!("sign excluded from payload  ");
    if payload("GET", "/p", &sample, "") == before {
        println!("ok");
        pass += 1;
    } else {
        println!("FAIL");
        fail += 1;
    }

    let mut blank = BTreeMap::new();
    blank.insert(String::from("x-param-opr"), String::new());
    blank.insert(String::from("x-param-cnt"), String::from("1"));
    print!("empty value skipped  ");
    if payload("GET", "/p", &blank, "") == "GET/px-param-cnt=1" {
        println!("ok");
        pass += 1;
    } else {
        println!("FAIL");
        fail += 1;
    }

    let mut upper = BTreeMap::new();
    upper.insert(String::from("X-Param-CNT"), String::from("1"));
    let mut lower = BTreeMap::new();
    lower.insert(String::from("x-param-cnt"), String::from("1"));
    print!("header name case folded  ");
    if payload("GET", "/p", &upper, "") == payload("GET", "/p", &lower, "") {
        println!("ok");
        pass += 1;
    } else {
        println!("FAIL");
        fail += 1;
    }

    let first = payload("POST", "/api/v1/login/check_uu_token", &base_params(1700000000), "{}");
    let second = payload("POST", "/api/v1/login/check_uu_token", &base_params(1700000001), "{}");
    print!("timestamp alters signature  ");
    if hmac(key, first.as_bytes()) != hmac(key, second.as_bytes()) {
        println!("ok");
        pass += 1;
    } else {
        println!("FAIL");
        fail += 1;
    }

    let mut owned = base_params(1700000000);
    owned.insert(String::from("x-param-user-id"), String::from("u1"));
    print!("user id alters signature  ");
    if hmac(key, first.as_bytes())
        != hmac(
            key,
            payload("POST", "/api/v1/login/check_uu_token", &owned, "{}").as_bytes(),
        )
    {
        println!("ok");
        pass += 1;
    } else {
        println!("FAIL");
        fail += 1;
    }

    print!("digest is 64 hex chars  ");
    if got_sign.len() == 64 && got_sign.chars().all(|c| c.is_ascii_hexdigit()) {
        println!("ok");
        pass += 1;
    } else {
        println!("FAIL");
        fail += 1;
    }

    println!("\n{} passed, {} failed", pass, fail);
    if fail > 0 {
        std::process::exit(1);
    }
}