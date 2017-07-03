use std::net::*;
use segment::*;

#[derive(Debug, Copy, Clone, PartialEq)]
enum State {
    LISTEN,
    SYN_SENT,
    SYN_RECD,
    ESTAB,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TCPTuple {
    src_port: u16,
    src_ip: IpAddr,
    dst_port: u16,
}

impl TCPTuple {
    pub fn from(s: &Segment, src: &SocketAddr) -> TCPTuple {
        TCPTuple {
            src_port: s.src_port(),
            dst_port: s.dst_port(),
            src_ip: src.ip(),
        }
    }
}

static mut TCB_COUNT: u32 = 0;

#[derive(Debug)]
pub struct TCB {
    id: u32,
    state: State,
    tuple: TCPTuple,
}

impl TCB {
    pub fn from(tuple: TCPTuple) -> TCB {
        unsafe {
            TCB_COUNT += 1;
        }
        TCB {
            id: unsafe { TCB_COUNT },
            state: State::LISTEN,
            tuple: tuple,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tcb() {
        let s = Segment::new();
        let src = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8080);
        let tuple = TCPTuple::from(&s, &src);
        let tcb = TCB::from(tuple);
        assert_eq!(tcb.id, 1);
        let tcb2 = TCB::from(tuple);
        assert_eq!(tcb2.id, 2);

        assert_eq!(tcb.state, State::LISTEN);
    }
}
