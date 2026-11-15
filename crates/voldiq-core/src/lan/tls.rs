use ring::{digest, rand::SystemRandom, signature::Ed25519KeyPair};
use rustls::{
    CertificateError, ClientConfig, ClientConnection, DigitallySignedStruct, Error, ServerConfig,
    ServerConnection, SignatureScheme, StreamOwned,
    client::{
        Resumption,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::{WebPkiSupportedAlgorithms, ring as tls_crypto, verify_tls13_signature_with_raw_key},
    pki_types::{
        CertificateDer, PrivatePkcs8KeyDer, ServerName, SubjectPublicKeyInfoDer, UnixTime,
    },
    server::AlwaysResolvesServerRawPublicKeys,
    sign::CertifiedKey,
    version::TLS13,
};
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) const TIMEOUT: Duration = Duration::from_secs(3);

fn fingerprint(key: &[u8]) -> [u8; 32] {
    let digest = digest::digest(&digest::SHA256, key);
    let mut bytes = [0; 32];
    bytes.copy_from_slice(digest.as_ref());
    bytes
}

pub(super) fn server() -> io::Result<(Arc<ServerConfig>, [u8; 32])> {
    let provider = Arc::new(tls_crypto::default_provider());
    let private = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
        .map_err(|_| io::Error::other("Cannot generate a local pairing key."))?;
    let key = tls_crypto::sign::any_eddsa_type(&PrivatePkcs8KeyDer::from(private.as_ref()))
        .map_err(io::Error::other)?;
    let public = key
        .public_key()
        .ok_or_else(|| io::Error::other("The local pairing key has no public key."))?;
    let pin = fingerprint(public.as_ref());
    let identity = CertifiedKey::new(vec![CertificateDer::from(public.as_ref().to_vec())], key);
    let resolver = AlwaysResolvesServerRawPublicKeys::new(Arc::new(identity));
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS13])
        .map_err(io::Error::other)?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(resolver));
    config.send_tls13_tickets = 0;
    Ok((Arc::new(config), pin))
}

#[derive(Debug)]
struct PairedKey {
    fingerprint: [u8; 32],
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PairedKey {
    fn verify_server_cert(
        &self,
        key: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _time: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if intermediates.is_empty() && fingerprint(key.as_ref()) == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::InvalidCertificate(CertificateError::UnknownIssuer))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _key: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("Local pairing requires TLS 1.3.".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        key: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature_with_raw_key(
            message,
            &SubjectPublicKeyInfoDer::from(key.as_ref()),
            signature,
            &self.algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

pub(super) fn accept(
    socket: TcpStream,
    config: Arc<ServerConfig>,
) -> io::Result<impl Read + Write> {
    let conn = ServerConnection::new(config).map_err(io::Error::other)?;
    Ok(StreamOwned::new(conn, DeadlineSocket::new(socket)))
}

pub(super) fn connect(address: SocketAddr, pin: [u8; 32]) -> io::Result<impl Read + Write> {
    let socket = DeadlineSocket::new(TcpStream::connect_timeout(&address, TIMEOUT)?);
    let provider = tls_crypto::default_provider();
    let verifier = PairedKey {
        fingerprint: pin,
        algorithms: provider.signature_verification_algorithms,
    };
    let mut config = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&TLS13])
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.resumption = Resumption::disabled();
    let conn = ClientConnection::new(Arc::new(config), ServerName::IpAddress(address.ip().into()))
        .map_err(io::Error::other)?;
    let mut stream = StreamOwned::new(conn, socket);
    // Send application credentials only after the pinned handshake has completed.
    stream.conn.complete_io(&mut stream.sock)?;
    Ok(stream)
}

struct DeadlineSocket {
    socket: TcpStream,
    end: Instant,
}

impl DeadlineSocket {
    fn new(socket: TcpStream) -> Self {
        Self {
            socket,
            end: Instant::now() + TIMEOUT,
        }
    }

    fn remaining(&self) -> io::Result<Duration> {
        self.end
            .checked_duration_since(Instant::now())
            .filter(|time| !time.is_zero())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "The local connection timed out.")
            })
    }
}

impl Read for DeadlineSocket {
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(data)
    }
}

impl Write for DeadlineSocket {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}
