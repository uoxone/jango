//! Tests for the listen socket and the epoll event layout.

use super::*;
use std::net::Ipv4Addr;

#[test]
fn epoll_event_matches_the_kernel_layout() {
    assert_eq!(std::mem::size_of::<EpollEvent>(), 12);
    assert_eq!(std::mem::align_of::<EpollEvent>(), 1);
    assert_eq!(std::mem::size_of::<SockAddrIn>(), 16);
}

#[test]
fn listen_socket_accepts_nothing_until_a_client() {
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
    let fd = listen_socket(addr).unwrap();
    nodelay(fd).unwrap();
    let err = accept(fd).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::WouldBlock);

    let ep = epoll_create().unwrap();
    epoll_add(ep, fd, LISTEN).unwrap();
    epoll_del(ep, fd).unwrap();
    close_fd(ep).unwrap();
    close_fd(fd).unwrap();
}
