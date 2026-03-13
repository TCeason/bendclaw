use std::future::Future;
use std::future::IntoFuture;

use anyhow::Context;
use tokio_util::sync::CancellationToken;

enum ServerExit {
    Api(std::io::Result<()>),
    Admin(std::io::Result<()>),
    Signal,
}

pub async fn supervise_servers<Api, Admin, Signal>(
    shutdown_token: CancellationToken,
    api_server: Api,
    admin_server: Option<Admin>,
    shutdown_signal: Signal,
) -> anyhow::Result<()>
where
    Api: IntoFuture<Output = std::io::Result<()>>,
    Admin: IntoFuture<Output = std::io::Result<()>>,
    Signal: Future<Output = ()>,
{
    let has_admin = admin_server.is_some();
    let api_server = async move { ServerExit::Api(api_server.into_future().await) };
    let admin_server = async move {
        match admin_server {
            Some(server) => ServerExit::Admin(server.into_future().await),
            None => std::future::pending::<ServerExit>().await,
        }
    };

    tokio::pin!(api_server);
    tokio::pin!(admin_server);
    tokio::pin!(shutdown_signal);

    let first_exit = tokio::select! {
        exit = &mut api_server => exit,
        exit = &mut admin_server => exit,
        _ = &mut shutdown_signal => {
            tracing::info!("shutdown signal received");
            ServerExit::Signal
        }
    };

    shutdown_token.cancel();

    let (api_result, admin_result) = match first_exit {
        ServerExit::Api(result) => {
            let admin_result = if has_admin {
                Some(expect_admin_exit(admin_server.await))
            } else {
                None
            };
            (result, admin_result)
        }
        ServerExit::Admin(result) => {
            let api_result = expect_api_exit(api_server.await);
            (api_result, Some(result))
        }
        ServerExit::Signal => {
            let api_result = expect_api_exit(api_server.await);
            let admin_result = if has_admin {
                Some(expect_admin_exit(admin_server.await))
            } else {
                None
            };
            (api_result, admin_result)
        }
    };

    api_result.context("api server error")?;
    if let Some(result) = admin_result {
        result.context("admin server error")?;
    }
    Ok(())
}

fn expect_api_exit(exit: ServerExit) -> std::io::Result<()> {
    match exit {
        ServerExit::Api(result) => result,
        ServerExit::Admin(_) | ServerExit::Signal => unreachable!("expected api server exit"),
    }
}

fn expect_admin_exit(exit: ServerExit) -> std::io::Result<()> {
    match exit {
        ServerExit::Admin(result) => result,
        ServerExit::Api(_) | ServerExit::Signal => unreachable!("expected admin server exit"),
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::future::ready;
    use std::io::Error;
    use std::io::ErrorKind;

    use tokio::sync::oneshot;
    use tokio_util::sync::CancellationToken;

    use super::*;

    #[tokio::test]
    async fn admin_exit_notifies_api_shutdown() {
        let shutdown_token = CancellationToken::new();
        let api_shutdown = shutdown_token.clone();
        let (api_stopped_tx, api_stopped_rx) = oneshot::channel();

        let api_server = async move {
            api_shutdown.cancelled().await;
            let _ = api_stopped_tx.send(());
            Ok(())
        };
        let admin_server = async { Err(Error::new(ErrorKind::Other, "admin failed")) };

        let result =
            supervise_servers(shutdown_token, api_server, Some(admin_server), pending()).await;

        assert!(result.is_err());
        api_stopped_rx.await.expect("api shutdown observed");
    }

    #[tokio::test]
    async fn api_exit_notifies_admin_shutdown() {
        let shutdown_token = CancellationToken::new();
        let admin_shutdown = shutdown_token.clone();
        let (admin_stopped_tx, admin_stopped_rx) = oneshot::channel();

        let api_server = async { Ok(()) };
        let admin_server = async move {
            admin_shutdown.cancelled().await;
            let _ = admin_stopped_tx.send(());
            Ok(())
        };

        supervise_servers(shutdown_token, api_server, Some(admin_server), pending())
            .await
            .expect("servers supervised");

        admin_stopped_rx.await.expect("admin shutdown observed");
    }

    #[tokio::test]
    async fn shutdown_signal_notifies_all_servers() {
        let shutdown_token = CancellationToken::new();
        let api_shutdown = shutdown_token.clone();
        let admin_shutdown = shutdown_token.clone();
        let (api_stopped_tx, api_stopped_rx) = oneshot::channel();
        let (admin_stopped_tx, admin_stopped_rx) = oneshot::channel();

        let api_server = async move {
            api_shutdown.cancelled().await;
            let _ = api_stopped_tx.send(());
            Ok(())
        };
        let admin_server = async move {
            admin_shutdown.cancelled().await;
            let _ = admin_stopped_tx.send(());
            Ok(())
        };

        supervise_servers(shutdown_token, api_server, Some(admin_server), ready(()))
            .await
            .expect("signal handled");

        api_stopped_rx.await.expect("api shutdown observed");
        admin_stopped_rx.await.expect("admin shutdown observed");
    }
}
