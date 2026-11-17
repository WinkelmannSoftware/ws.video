//! Bildschirmaufnahme über das Desktop-Portal (xdg-desktop-portal, ScreenCast) und PipeWire.
//! Das ist der offizielle Weg unter Wayland; er funktioniert auf jedem Monitor und jeder Grafikkarte.
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::{PersistMode, Session};
use std::os::fd::OwnedFd;

pub struct ScreenSource {
    proxy: Screencast,
    session: Session<Screencast>,
    pub node: u32,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
    pub cursor_hidden: bool,
    pub restore_token: Option<String>,
}

impl ScreenSource {
    /// Öffnet die Auswahl des Systems (Monitor, Fenster oder Bereich). Mit `restore` wird die letzte Auswahl
    /// wiederverwendet, sofern das Portal das erlaubt.
    pub async fn open(hide_cursor: bool, restore: Option<String>) -> anyhow::Result<ScreenSource> {
        let proxy = Screencast::new().await.map_err(|e| anyhow::anyhow!("Das Desktop-Portal für Bildschirmaufnahmen ist nicht erreichbar ({e}). Läuft xdg-desktop-portal-hyprland?"))?;
        let session = proxy.create_session(Default::default()).await?;
        let modes = proxy.available_cursor_modes().await.unwrap_or(CursorMode::EMBEDDED);
        let cursor_hidden = hide_cursor && modes.contains(CursorMode::HIDDEN);
        let cursor = if cursor_hidden { CursorMode::HIDDEN } else if modes.contains(CursorMode::EMBEDDED) { CursorMode::EMBEDDED } else { CursorMode::HIDDEN };
        let types = proxy.available_source_types().await.unwrap_or(SourceType::MONITOR);
        let mut opts = SelectSourcesOptions::default()
            .set_cursor_mode(cursor)
            .set_sources(types & (SourceType::MONITOR | SourceType::WINDOW))
            .set_multiple(false)
            .set_persist_mode(PersistMode::ExplicitlyRevoked);
        if let Some(t) = restore.as_deref() {
            if !t.is_empty() { opts = opts.set_restore_token(t); }
        }
        proxy.select_sources(&session, opts).await?;
        let response = proxy
            .start(&session, None, Default::default())
            .await?
            .response()
            .map_err(|e| anyhow::anyhow!("Die Auswahl wurde abgebrochen ({e})."))?;
        let stream = response.streams().first().ok_or_else(|| anyhow::anyhow!("Das Portal hat keine Quelle geliefert."))?;
        let cursor_really_hidden = cursor_hidden;
        Ok(ScreenSource {
            node: stream.pipe_wire_node_id(),
            size: stream.size(),
            position: stream.position(),
            cursor_hidden: cursor_really_hidden,
            restore_token: response.restore_token().map(|s| s.to_string()),
            proxy,
            session,
        })
    }

    /// Neue Verbindung zu PipeWire für eine Pipeline.
    pub async fn fd(&self) -> anyhow::Result<OwnedFd> {
        Ok(self.proxy.open_pipe_wire_remote(&self.session, Default::default()).await?)
    }

    pub async fn close(&self) {
        let _ = self.session.close().await;
    }

    pub fn describe(&self) -> String {
        match self.size {
            Some((w, h)) => format!("{w} × {h}"),
            None => "Quelle gewählt".to_string(),
        }
    }
}
