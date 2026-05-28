/**
 * Gateway/Channels settings panel — WeChat & WeCom channel configuration.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { MessageSquare, Play, Power, RefreshCw, QrCode, LogOut } from "lucide-react";
import { useT } from "@/i18n";
import { Button } from "@/shared/ui/button";

interface GatewayStatus {
  running: boolean;
  pid: number | null;
}

interface WeixinSession {
  token: string;
  account_id: string;
  created_at: number;
}

export function GatewaySettingsPanel({ description }: { description: string }) {
  const t = useT();
  const [status, setStatus] = useState<GatewayStatus | null>(null);
  const [weixinSession, setWeixinSession] = useState<WeixinSession | null>(null);
  const [loading, setLoading] = useState(false);
  const [qrImage, setQrImage] = useState<string | null>(null);
  const [loginPolling, setLoginPolling] = useState(false);

  // Fetch gateway status and weixin session on mount.
  useEffect(() => {
    if (!isTauri()) return;
    refreshStatus();
    refreshWeixinSession();
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      const s = await invoke<GatewayStatus>("gateway_status");
      setStatus(s);
    } catch {
      setStatus(null);
    }
  }, []);

  const refreshWeixinSession = useCallback(async () => {
    try {
      const s = await invoke<WeixinSession | null>("gateway_weixin_session");
      setWeixinSession(s);
    } catch {
      setWeixinSession(null);
    }
  }, []);

  const handleStart = async () => {
    setLoading(true);
    try {
      await invoke("gateway_start");
      await refreshStatus();
    } finally {
      setLoading(false);
    }
  };

  const handleStop = async () => {
    setLoading(true);
    try {
      await invoke("gateway_stop");
      await refreshStatus();
    } finally {
      setLoading(false);
    }
  };

  const handleRestart = async () => {
    setLoading(true);
    try {
      await invoke("gateway_restart");
      await refreshStatus();
    } finally {
      setLoading(false);
    }
  };

  const handleQrLogin = async () => {
    setLoading(true);
    setQrImage(null);
    try {
      const result = await invoke<{
        qr_image_base64: string | null;
        qr_url: string | null;
        login_uuid: string;
      }>("gateway_weixin_qr_login", { baseUrl: null });
      if (result.qr_image_base64) {
        setQrImage(result.qr_image_base64);
      }
      setLoginPolling(true);
      pollLogin(result.login_uuid);
    } catch (e) {
      console.error("QR login failed:", e);
    } finally {
      setLoading(false);
    }
  };

  const pollLogin = async (uuid: string) => {
    for (let i = 0; i < 60; i++) {
      await new Promise((r) => setTimeout(r, 3000));
      try {
        const result = await invoke<{ status: string; session?: WeixinSession }>(
          "gateway_weixin_login_poll",
          { loginUuid: uuid, baseUrl: null }
        );
        if (result.status === "success") {
          setWeixinSession(result.session ?? null);
          setQrImage(null);
          setLoginPolling(false);
          return;
        }
        if (result.status === "expired" || result.status === "failed") {
          setQrImage(null);
          setLoginPolling(false);
          return;
        }
      } catch {
        setLoginPolling(false);
        return;
      }
    }
    setLoginPolling(false);
  };

  const handleLogout = async () => {
    await invoke("gateway_weixin_logout");
    setWeixinSession(null);
  };

  return (
    <div className="flex flex-col gap-6">
      {/* Header */}
      <div className="flex flex-col gap-1">
        <h2 className="text-lg font-semibold">{t("settings.category.gateway")}</h2>
        <p className="text-sm text-muted-foreground">{description}</p>
      </div>

      {/* Gateway Status & Controls */}
      <div className="rounded-lg border bg-card p-4 flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <MessageSquare className="h-4 w-4 text-muted-foreground" />
            <span className="text-sm font-medium">{t("settings.gateway.status")}</span>
          </div>
          <div className="flex items-center gap-2">
            <span
              className={`inline-flex items-center gap-1.5 text-xs font-medium px-2 py-0.5 rounded-full ${
                status?.running
                  ? "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400"
                  : "bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400"
              }`}
            >
              <span
                className={`h-1.5 w-1.5 rounded-full ${
                  status?.running ? "bg-green-500" : "bg-zinc-400"
                }`}
              />
              {status?.running ? t("settings.gateway.running") : t("settings.gateway.stopped")}
            </span>
          </div>
        </div>

        <div className="flex gap-2">
          {!status?.running ? (
            <Button size="sm" onClick={handleStart} disabled={loading}>
              <Play className="h-3.5 w-3.5 mr-1" />
              {t("settings.gateway.start")}
            </Button>
          ) : (
            <>
              <Button size="sm" variant="outline" onClick={handleStop} disabled={loading}>
                <Power className="h-3.5 w-3.5 mr-1" />
                {t("settings.gateway.stop")}
              </Button>
              <Button size="sm" variant="outline" onClick={handleRestart} disabled={loading}>
                <RefreshCw className="h-3.5 w-3.5 mr-1" />
                {t("settings.gateway.restart")}
              </Button>
            </>
          )}
        </div>
      </div>

      {/* WeChat Login Section */}
      <div className="rounded-lg border bg-card p-4 flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <div className="flex flex-col gap-0.5">
            <span className="text-sm font-medium">{t("settings.gateway.weixinLogin")}</span>
            <span className="text-xs text-muted-foreground">
              {t("settings.gateway.weixinLoginDesc")}
            </span>
          </div>
          <span
            className={`text-xs font-medium ${
              weixinSession ? "text-green-600 dark:text-green-400" : "text-muted-foreground"
            }`}
          >
            {weixinSession ? t("settings.gateway.loggedIn") : t("settings.gateway.notLoggedIn")}
          </span>
        </div>

        {weixinSession ? (
          <div className="flex items-center justify-between">
            <span className="text-xs text-muted-foreground font-mono">
              {weixinSession.account_id || "—"}
            </span>
            <Button size="sm" variant="ghost" onClick={handleLogout}>
              <LogOut className="h-3.5 w-3.5 mr-1" />
              {t("settings.gateway.logout")}
            </Button>
          </div>
        ) : (
          <div className="flex flex-col items-center gap-3">
            {qrImage ? (
              <img
                src={`data:image/png;base64,${qrImage}`}
                alt="QR Code"
                className="w-48 h-48 rounded border"
              />
            ) : null}
            {loginPolling ? (
              <span className="text-xs text-muted-foreground animate-pulse">
                Waiting for scan...
              </span>
            ) : (
              <Button size="sm" onClick={handleQrLogin} disabled={loading}>
                <QrCode className="h-3.5 w-3.5 mr-1" />
                {t("settings.gateway.scanQr")}
              </Button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
