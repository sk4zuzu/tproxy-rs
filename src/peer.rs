use crate::{
    self as tproxy,
    Result,
};

// ---

use std::io::ErrorKind::ConnectionRefused;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::fs::{self};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
use tokio::{select, spawn};
use tokio_util::sync::CancellationToken;

// ---

pub trait Peer<I, O>: Send
where
    I: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static,
    O: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static,
{
    fn new_ctoken(&mut self) -> CancellationToken;

    fn new_incoming_stream(&mut self) -> impl Future<Output = Result<I>> + Send;
    fn new_outgoing_stream(&mut self) -> impl Future<Output = Result<O>> + Send;

    fn run(&mut self) -> impl Future<Output = Result<()>> + Send {
        async {
            let ctoken1 = self.new_ctoken();

            loop {
                let mut incoming = loop {
                    select! {
                        _ = ctoken1.cancelled() => return Ok(()),
                        r = self.new_incoming_stream() => match r {
                            Ok(v) => break v, Err(e) => return Err(e),
                        }
                    }
                };

                let mut outgoing = loop {
                    select! {
                        _ = ctoken1.cancelled() => return Ok(()),
                        r = self.new_outgoing_stream() => match r {
                            Ok(v) => break v, Err(e) => return Err(e),
                        }
                    }
                };

                let ctoken2 = self.new_ctoken();

                spawn(async move {
                    loop {
                        select! {
                            _ = ctoken2.cancelled() => {
                                // gracefully shutdown both streams to prevent file descriptor leaks
                                incoming.shutdown().await.ok();
                                outgoing.shutdown().await.ok();
                                return Ok(())
                            },
                            r = Self::glue_streams(&mut incoming, &mut outgoing) => break r,
                        }
                    }.map_err(tproxy::log_err)
                });
            }
        }
    }

    fn glue_streams(incoming: &mut I, outgoing: &mut O) -> impl Future<Output = Result<()>> + Send {
        async {
            let (mut incoming_ro, mut incoming_wo) = io::split(&mut *incoming);
            let (mut outgoing_ro, mut outgoing_wo) = io::split(&mut *outgoing);

            let mut incoming_done = false;
            let mut outgoing_done = false;

            loop {
                select! {
                    r = io::copy(&mut incoming_ro, &mut outgoing_wo), if !incoming_done => {
                        if let Err(e) = r { break Err(e.into()) } else { incoming_done = true }
                    }
                    r = io::copy(&mut outgoing_ro, &mut incoming_wo), if !outgoing_done => {
                        if let Err(e) = r { break Err(e.into()) } else { outgoing_done = true }
                    }
                }

                if incoming_done || outgoing_done {
                    // gracefully shutdown both streams to prevent file descriptor leaks
                    incoming.shutdown().await.ok();
                    outgoing.shutdown().await.ok();
                    break Ok(())
                }
            }
        }
    }
}

// ---

pub struct InnerPeer {
    pub incoming_addr: SocketAddr,
    pub outgoing_addr: PathBuf,
    ctoken: CancellationToken,
    listener: Option<TcpListener>,
}

impl InnerPeer {
    pub fn new(incoming_addr: SocketAddr, outgoing_addr: PathBuf, ctoken: CancellationToken) -> Self {
        Self { incoming_addr, outgoing_addr, ctoken, listener: None }
    }
}

impl Peer<TcpStream, UnixStream> for InnerPeer {
    fn new_ctoken(&mut self) -> CancellationToken { self.ctoken.clone() }

    fn new_incoming_stream(&mut self) -> impl Future<Output = Result<TcpStream>> + Send {
        async {
            if let None = self.listener {
                self.listener = Some(TcpListener::bind(self.incoming_addr).await?);
            };

            match self.listener.as_ref().unwrap().accept().await {
                Ok((stream, _)) => Ok(stream), Err(e) => Err(e.into()),
            }
        }
    }

    fn new_outgoing_stream(&mut self) -> impl Future<Output = Result<UnixStream>> + Send {
        async {
            match UnixStream::connect(self.outgoing_addr.as_path()).await {
                Ok(stream) => Ok(stream), Err(e) => Err(e.into()),
            }
        }
    }
}

// ---

pub struct OuterPeer {
    pub incoming_addr: PathBuf,
    pub outgoing_addr: SocketAddr,
    ctoken: CancellationToken,
    listener: Option<UnixListener>,
}

impl OuterPeer {
    pub fn new(incoming_addr: PathBuf, outgoing_addr: SocketAddr, ctoken: CancellationToken) -> Self {
        Self { incoming_addr, outgoing_addr, ctoken, listener: None }
    }
}

impl Peer<UnixStream, TcpStream> for OuterPeer {
    fn new_ctoken(&mut self) -> CancellationToken { self.ctoken.clone() }

    fn new_incoming_stream(&mut self) -> impl Future<Output = Result<UnixStream>> + Send {
        async {
            if let None = self.listener {
                let p = self.incoming_addr.as_path();
                // remove the socket file when unbound, but also let it fail later during bind
                match UnixStream::connect(p).await {
                    Ok(mut stream) => stream.shutdown().await.ok(),
                    Err(e) if e.kind() == ConnectionRefused => fs::remove_file(p).await.ok(),
                    Err(_) => None,
                };
                self.listener = Some(UnixListener::bind(p)?);
            };

            match self.listener.as_ref().unwrap().accept().await {
                Ok((stream, _)) => Ok(stream), Err(e) => Err(e.into()),
            }
        }
    }

    fn new_outgoing_stream(&mut self) -> impl Future<Output = Result<TcpStream>> + Send {
        async {
            match TcpStream::connect(self.outgoing_addr).await {
                Ok(stream) => Ok(stream), Err(e) => Err(e.into()),
            }
        }
    }
}
