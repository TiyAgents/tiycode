/**
 * Gateway/Channels settings panel — WeChat & WeCom channel configuration.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { AlertCircle, Play, Power, RefreshCw, QrCode, LogOut, Radio, Save } from "lucide-react";
import { useT } from "@/i18n";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Switch } from "@/shared/ui/switch";

interface GatewayStatus {
  running: boolean;
  pid: number | null;
  version: string | null;
  configExists: boolean;
}

interface WeixinSession {
  token: string;
  account_id: string;
  created_at: number;
}

interface GatewayConfigDto {
  platform: string;
  weixinEnabled: boolean;
  wecomEnabled: boolean;
  wecomBotId: string;
  wecomSecretSet: boolean;
  wecomWsUrl: string;
}

export function GatewaySettingsPanel({ description }: { description: string }) {
  const t = useT();
  const [status, setStatus] = useState<GatewayStatus | null>(null);
  const [weixinSession, setWeixinSession] = useState<WeixinSession | null>(null);
  const [loading, setLoading] = useState(false);
  const [qrImage, setQrImage] = useState<string | null>(null);
  const [loginPolling, setLoginPolling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Config form state.
  const [weixinEnabled, setWeixinEnabled] = useState(false);
  const [wecomEnabled, setWecomEnabled] = useState(false);
  const [wecomBotId, setWecomBotId] = useState("");
  const [wecomSecret, setWecomSecret] = useState("");
  const [wecomSecretSet, setWecomSecretSet] = useState(false);
  const [wecomWsUrl, setWecomWsUrl] = useState("openws.work.weixin.qq.com");
  const [configDirty, setConfigDirty] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    refreshStatus();
    refreshWeixinSession();
    loadConfig();
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

  const loadConfig = useCallback(async () => {
    try {
      const cfg = await invoke<GatewayConfigDto | null>("gateway_get_config");
      if (cfg) {
        setWeixinEnabled(cfg.weixinEnabled);
        setWecomEnabled(cfg.wecomEnabled);
        setWecomBotId(cfg.wecomBotId);
        setWecomSecretSet(cfg.wecomSecretSet);
        setWecomWsUrl(cfg.wecomWsUrl || "openws.work.weixin.qq.com");
      }
    } catch {
      // Config doesn't exist yet — use defaults.
    }
    setConfigDirty(false);
  }, []);

  const handleSaveConfig = async () => {
    setSaving(true);
    setError(null);
    try {
      await invoke("gateway_save_config", {
        input: {
          weixinEnabled,
          wecomEnabled,
          wecomBotId,
          wecomSecret,
          wecomWsUrl,
        },
      });
      setConfigDirty(false);
      setWecomSecret("");
      if (wecomSecret) setWecomSecretSet(true);
      await refreshStatus();
    } catch (e) {
      setError(`Save failed: ${String(e)}`);
    } finally {
      setSaving(false);
    }
  };

  const markDirty = () => setConfigDirty(true);

  const handleStart = async () => {
    setLoading(true);
    setError(null);
    try {
      await invoke("gateway_start");
      await refreshStatus();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleStop = async () => {
    setLoading(true);
    setError(null);
    try {
      await invoke("gateway_stop");
      await refreshStatus();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleRestart = async () => {
    setLoading(true);
    setError(null);
    try {
      await invoke("gateway_restart");
      await refreshStatus();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleQrLogin = async () => {
    setLoading(true);
    setQrImage(null);
    setError(null);
    try {
      const result = await invoke<{
        qr_image_base64: string | null;
        qr_url: string | null;
        login_uuid: string;
        media_type: string | null;
      }>("gateway_weixin_qr_login", { baseUrl: null });
      if (result.qr_image_base64) {
        const mime = result.media_type === "svg+xml" ? "image/svg+xml" : result.media_type === "jpeg" ? "image/jpeg" : "image/png";
        setQrImage(`data:${mime};base64,${result.qr_image_base64}`);
      } else if (result.qr_url) {
        setError(`QR URL: ${result.qr_url}`);
      }
      if (result.login_uuid) {
        setLoginPolling(true);
        pollLogin(result.login_uuid);
      }
    } catch (e) {
      setError(`QR login failed: ${String(e)}`);
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
        if (result.status === "waiting_confirm" || result.status === "scanned_redirect") {
          // Keep polling — user scanned or redirecting.
          continue;
        }
        if (result.status === "expired" || result.status === "failed") {
          setQrImage(null);
          setLoginPolling(false);
          setError("QR code expired or login failed. Please try again.");
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

      {/* Error Banner */}
      {error && (
        <div className="flex items-start gap-2 rounded-lg border border-red-200 dark:border-red-800/50 bg-red-50 dark:bg-red-950/20 p-3">
          <AlertCircle className="h-4 w-4 text-red-500 mt-0.5 shrink-0" />
          <p className="text-xs text-red-700 dark:text-red-400 break-all">{error}</p>
        </div>
      )}

      {/* Gateway Process Status */}
      <div className="rounded-lg border bg-card p-4 flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Radio className="h-4 w-4 text-muted-foreground" />
            <span className="text-sm font-medium">Gateway Status</span>
          </div>
          <span
            className={`inline-flex items-center gap-1.5 text-xs font-medium px-2 py-0.5 rounded-full ${
              status?.running
                ? "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400"
                : "bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400"
            }`}
          >
            <span className={`h-1.5 w-1.5 rounded-full ${status?.running ? "bg-green-500" : "bg-zinc-400"}`} />
            {status?.running ? t("settings.gateway.running") : t("settings.gateway.stopped")}
          </span>
        </div>

        {status && (
          <div className="grid grid-cols-3 gap-3 text-xs">
            <div className="flex flex-col gap-0.5">
              <span className="text-muted-foreground">PID</span>
              <span className="font-mono font-medium">{status.running && status.pid ? status.pid : "—"}</span>
            </div>
            <div className="flex flex-col gap-0.5">
              <span className="text-muted-foreground">Version</span>
              <span className="font-mono font-medium">{status.running && status.version ? `v${status.version}` : "—"}</span>
            </div>
            <div className="flex flex-col gap-0.5">
              <span className="text-muted-foreground">Config</span>
              <span className={`font-medium ${status.configExists ? "text-green-600 dark:text-green-400" : "text-amber-600 dark:text-amber-400"}`}>
                {status.configExists ? "✓ Found" : "✗ Missing"}
              </span>
            </div>
          </div>
        )}

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

      {/* WeChat Channel */}
      <div className="rounded-lg border bg-card p-4 flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <div className="flex flex-col gap-0.5">
            <span className="text-sm font-medium">{t("settings.gateway.weixin")}</span>
            <span className="text-xs text-muted-foreground">{t("settings.gateway.weixinLoginDesc")}</span>
          </div>
          <div className="flex items-center gap-3">
            <span className={`text-xs font-medium ${weixinSession ? "text-green-600 dark:text-green-400" : "text-muted-foreground"}`}>
              {weixinSession ? t("settings.gateway.loggedIn") : t("settings.gateway.notLoggedIn")}
            </span>
            <Switch
              checked={weixinEnabled}
              onCheckedChange={(v) => { setWeixinEnabled(v); markDirty(); }}
            />
          </div>
        </div>

        {weixinEnabled && (
          <>
            {weixinSession ? (
              <div className="flex items-center justify-between">
                <span className="text-xs text-muted-foreground font-mono">Account: {weixinSession.account_id || "—"}</span>
                <Button size="sm" variant="ghost" onClick={handleLogout}>
                  <LogOut className="h-3.5 w-3.5 mr-1" />
                  {t("settings.gateway.logout")}
                </Button>
              </div>
            ) : (
              <div className="flex flex-col items-center gap-3">
                {qrImage && <img src={qrImage} alt="QR Code" className="w-48 h-48 rounded border" />}
                {loginPolling ? (
                  <span className="text-xs text-muted-foreground animate-pulse">Waiting for scan...</span>
                ) : (
                  <Button size="sm" onClick={handleQrLogin} disabled={loading}>
                    <QrCode className="h-3.5 w-3.5 mr-1" />
                    {loading ? "Loading..." : t("settings.gateway.scanQr")}
                  </Button>
                )}
              </div>
            )}
          </>
        )}
      </div>

      {/* WeCom Channel */}
      <div className="rounded-lg border bg-card p-4 flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <div className="flex flex-col gap-0.5">
            <span className="text-sm font-medium">{t("settings.gateway.wecom")}</span>
            <span className="text-xs text-muted-foreground">WebSocket connection to WeCom AI Bot platform.</span>
          </div>
          <Switch
            checked={wecomEnabled}
            onCheckedChange={(v) => { setWecomEnabled(v); markDirty(); }}
          />
        </div>

        {wecomEnabled && (
          <div className="flex flex-col gap-3">
            <div className="flex flex-col gap-1">
              <label className="text-xs text-muted-foreground">{t("settings.gateway.wecomBotId")}</label>
              <Input
                value={wecomBotId}
                onChange={(e) => { setWecomBotId(e.target.value); markDirty(); }}
                placeholder="Enter Bot ID"
                className="h-8 text-sm font-mono"
              />
            </div>
            <div className="flex flex-col gap-1">
              <label className="text-xs text-muted-foreground">{t("settings.gateway.wecomSecret")}</label>
              <Input
                type="password"
                value={wecomSecret}
                onChange={(e) => { setWecomSecret(e.target.value); markDirty(); }}
                placeholder={wecomSecretSet ? "••••••••  (unchanged)" : "Enter Secret"}
                className="h-8 text-sm font-mono"
              />
            </div>
            <div className="flex flex-col gap-1">
              <label className="text-xs text-muted-foreground">{t("settings.gateway.wecomWsUrl")}</label>
              <Input
                value={wecomWsUrl}
                onChange={(e) => { setWecomWsUrl(e.target.value); markDirty(); }}
                placeholder="openws.work.weixin.qq.com"
                className="h-8 text-sm font-mono"
              />
            </div>
          </div>
        )}
      </div>

      {/* Save Button */}
      {configDirty && (
        <div className="flex justify-end">
          <Button size="sm" onClick={handleSaveConfig} disabled={saving}>
            <Save className="h-3.5 w-3.5 mr-1" />
            {saving ? "Saving..." : t("settings.gateway.save")}
          </Button>
        </div>
      )}
    </div>
  );
}
