use crate::base::ErrorCode;
use crate::base::HttpErrorKind;
use crate::base::HttpTransportError;

pub(crate) fn to_storage_error(operation: &str, err: HttpTransportError) -> ErrorCode {
    match err.kind {
        HttpErrorKind::RequestTimeout => ErrorCode::timeout(format!("{operation}: {err}")),
        HttpErrorKind::DnsFailure
        | HttpErrorKind::TcpConnectFailure
        | HttpErrorKind::TlsHandshakeFailure
        | HttpErrorKind::ProxyInterrupted
        | HttpErrorKind::ConnectionInterrupted => {
            ErrorCode::storage_connection(format!("{operation}: {err}"))
        }
        HttpErrorKind::InvalidRequest | HttpErrorKind::InvalidResponse | HttpErrorKind::Unknown => {
            ErrorCode::storage_exec(format!("{operation}: {err}"))
        }
    }
}
