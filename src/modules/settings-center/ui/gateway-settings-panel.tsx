/**
 * Gateway/Channels settings panel — WeChat & WeCom channel configuration.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { AlertCircle, Play, RefreshCw, QrCode, LogOut, Save } from "lucide-react";
import { useT } from "@/i18n";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Switch } from "@/shared/ui/switch";
import { PageHeading, SettingsSection, SettingsRow, SectionDivider } from "./settings-shared";

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
  const [sessionExpired, setSessionExpired] = useState(false);
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

  // Detect session expiry: session was present but is now null while gateway is still running.
  const prevSessionRef = useRef<WeixinSession | null>(null);
  useEffect(() => {
    if (prevSessionRef.current !== null && weixinSession === null && status?.running) {
      setSessionExpired(true);
    }
    prevSessionRef.current = weixinSession;
  }, [weixinSession, status?.running]);

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

  const autoSaveConfig = useCallback(async (overrides: Partial<{
    weixinEnabled: boolean;
    wecomEnabled: boolean;
    wecomBotId: string;
    wecomSecret: string;
    wecomWsUrl: string;
  }>) => {
    try {
      await invoke("gateway_save_config", {
        input: {
          weixinEnabled,
          wecomEnabled,
          wecomBotId,
          wecomSecret: "",
          wecomWsUrl,
          ...overrides,
        },
      });
      setConfigDirty(false);
    } catch (e) {
      setError(`Save failed: ${String(e)}`);
    }
  }, [weixinEnabled, wecomEnabled, wecomBotId, wecomWsUrl]);

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
          setSessionExpired(false);
          try {
            setWeixinEnabled(true);
            await invoke("gateway_save_config", {
              input: {
                weixinEnabled: true,
                wecomEnabled,
                wecomBotId,
                wecomSecret: "",
                wecomWsUrl,
              },
            });
            setConfigDirty(false);
          } catch {
            // Non-fatal — user can still manually save.
          }
          await refreshStatus();
          return;
        }
        if (result.status === "waiting_confirm" || result.status === "scanned_redirect") {
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
    setSessionExpired(false);
  };

  return (
    <div className="flex flex-col gap-6">
      <PageHeading title={t("settings.category.gateway")} description={description} />

      {/* Error Banner */}
      {error && (
        <div className="flex items-start gap-2 rounded-xl border border-app-border bg-app-danger/5 p-3">
          <AlertCircle className="h-4 w-4 text-app-danger mt-0.5 shrink-0" />
          <p className="text-xs text-app-danger break-all">{error}</p>
        </div>
      )}

      {/* Gateway Process Status */}
      <SettingsSection title={t("settings.gateway.status")}>
        <SettingsRow
          label={status?.running ? t("settings.gateway.running") : t("settings.gateway.stopped")}
          description={
            status?.running
              ? `PID ${status.pid ?? "—"} · ${status.version ? `v${status.version}` : "—"} · Config ${status.configExists ? "✓" : "✗"}`
              : "Gateway process is not running."
          }
          control={
            <div className="flex items-center gap-2">
              <span
                className={`inline-flex items-center gap-1.5 text-xs font-medium px-2 py-0.5 rounded-full ${
                  status?.running
                    ? "bg-green-500/10 text-green-600 dark:text-green-400"
                    : "bg-app-surface text-app-muted"
                }`}
              >
                <span
                  className={`h-1.5 w-1.5 rounded-full ${status?.running ? "bg-green-500" : "bg-app-subtle"}`}
                />
                {status?.running ? t("settings.gateway.running") : t("settings.gateway.stopped")}
              </span>
              {!status?.running ? (
                <Button size="sm" onClick={handleStart} disabled={loading}>
                  <Play className="h-3.5 w-3.5 mr-1" />
                  {t("settings.gateway.start")}
                </Button>
              ) : (
                <Button size="sm" variant="outline" onClick={handleRestart} disabled={loading}>
                  <RefreshCw className="h-3.5 w-3.5 mr-1" />
                  {t("settings.gateway.restart")}
                </Button>
              )}
            </div>
          }
        />
      </SettingsSection>

      {/* WeChat Channel */}
      <SettingsSection title={t("settings.gateway.weixin")}>
        <SettingsRow
          label={t("settings.gateway.weixin")}
          description={t("settings.gateway.weixinLoginDesc")}
          control={
            <div className="flex items-center gap-3">
              <span
                className={`text-xs font-medium ${
                  weixinSession
                    ? "text-green-600 dark:text-green-400"
                    : sessionExpired
                      ? "text-amber-600 dark:text-amber-400"
                      : "text-app-muted"
                }`}
              >
                {weixinSession
                  ? t("settings.gateway.loggedIn")
                  : sessionExpired
                    ? t("settings.gateway.sessionExpired")
                    : t("settings.gateway.notLoggedIn")}
              </span>
              <Switch
                size="sm"
                checked={weixinEnabled}
                onCheckedChange={(v) => {
                  setWeixinEnabled(v);
                  autoSaveConfig({ weixinEnabled: v });
                }}
              />
            </div>
          }
        />
        {weixinEnabled && (
          <>
            <SectionDivider />
            {weixinSession ? (
              <SettingsRow
                label={weixinSession.account_id || "—"}
                description={`Token: ${(weixinSession.token ?? "").slice(0, 8)}…`}
                control={
                  <Button size="sm" variant="ghost" onClick={handleLogout}>
                    <LogOut className="h-3.5 w-3.5 mr-1" />
                    {t("settings.gateway.logout")}
                  </Button>
                }
              />
            ) : (
              <div className="flex flex-col items-center gap-4 px-4 py-6">
                {sessionExpired && (
                  <div className="flex items-center gap-2 w-full rounded-lg border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 px-3 py-2.5">
                    <AlertCircle className="h-4 w-4 text-amber-600 dark:text-amber-400 shrink-0" />
                    <span className="text-xs text-amber-700 dark:text-amber-300">
                      {t("settings.gateway.sessionExpiredDesc")}
                    </span>
                  </div>
                )}
                {qrImage && <img src={qrImage} alt="QR Code" className="w-48 h-48 rounded-lg border border-app-border" />}
                {loginPolling ? (
                  <span className="text-xs text-app-muted animate-pulse">Waiting for scan...</span>
                ) : (
                  <Button size="sm" onClick={handleQrLogin} disabled={loading}>
                    <QrCode className="h-3.5 w-3.5 mr-1" />
                    {loading ? "Loading..." : sessionExpired ? t("settings.gateway.reLogin") : t("settings.gateway.scanQr")}
                  </Button>
                )}
              </div>
            )}
          </>
        )}
      </SettingsSection>

      {/* WeCom Channel */}
      <SettingsSection title={t("settings.gateway.wecom")}>
        <SettingsRow
          label={t("settings.gateway.wecom")}
          description="WebSocket connection to WeCom AI Bot platform."
          control={
            <Switch
              size="sm"
              checked={wecomEnabled}
              onCheckedChange={(v) => {
                setWecomEnabled(v);
                autoSaveConfig({ wecomEnabled: v });
              }}
            />
          }
        />
        {wecomEnabled && (
          <>
            <SectionDivider />
            <SettingsRow
              label={t("settings.gateway.wecomBotId")}
              description="The Bot ID from WeCom AI Bot platform."
              control={
                <div className="flex w-full flex-col gap-2 md:w-[360px]">
                  <Input
                    value={wecomBotId}
                    onChange={(e) => {
                      setWecomBotId(e.target.value);
                      markDirty();
                    }}
                    placeholder="Enter Bot ID"
                    className="font-mono"
                  />
                </div>
              }
            />
            <SectionDivider />
            <SettingsRow
              label={t("settings.gateway.wecomSecret")}
              description={wecomSecretSet ? "Secret is configured. Leave empty to keep unchanged." : "Enter the Bot Secret."}
              control={
                <div className="flex w-full flex-col gap-2 md:w-[360px]">
                  <Input
                    type="password"
                    value={wecomSecret}
                    onChange={(e) => {
                      setWecomSecret(e.target.value);
                      markDirty();
                    }}
                    placeholder={wecomSecretSet ? "••••••••  (unchanged)" : "Enter Secret"}
                    className="font-mono"
                  />
                </div>
              }
            />
            {configDirty && (
              <>
                <SectionDivider />
                <div className="flex justify-end px-4 py-3">
                  <Button size="sm" onClick={handleSaveConfig} disabled={saving}>
                    <Save className="h-3.5 w-3.5 mr-1" />
                    {saving ? "Saving..." : t("settings.gateway.save")}
                  </Button>
                </div>
              </>
            )}
          </>
        )}
      </SettingsSection>
    </div>
  );
}
