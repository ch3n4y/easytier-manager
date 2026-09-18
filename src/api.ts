import {invoke} from '@tauri-apps/api/core';

export type Status = {
  installed: boolean;
  running: boolean;
  loaded: boolean;
  /** Version of the installed EasyTier core, empty when it is not installed. */
  version: string;
  mode: string;
  pid: number;
  adminReady: boolean;
  adminError: string;
  /** Host platform, used to tailor the authorization copy. */
  platform: string;
  /** Version of this app, which is what the updater compares releases against. */
  appVersion: string;
};

export type ConfigPayload = {
  mode: string;
  configServer: string;
  defaultConf: string;
};

export type UpdateInfo = {
  installed: boolean;
  currentVersion: string;
  latestVersion: string;
  hasUpdate: boolean;
  assetName: string;
};

/** The manager's own release channel, which is separate from the core's. */
export type AppUpdateInfo = {
  currentVersion: string;
  latestVersion: string;
  hasUpdate: boolean;
  /** Release notes, empty when there is nothing to install. */
  notes: string;
  /** Publication date of the release, empty when the manifest omits it. */
  date: string;
};

export type AppSettings = {
  /** Prefix prepended to GitHub URLs when downloading release assets. */
  githubProxy: string;
};

export type DownloadProgress = {
  received: number;
  total: number | null;
};

export const emptyStatus: Status = {
  installed: false,
  running: false,
  loaded: false,
  version: '',
  mode: 'web',
  pid: 0,
  adminReady: false,
  adminError: '',
  platform: '',
  appVersion: '',
};

export const GetStatus = () => invoke<Status>('get_status');

export const InstallLatest = () => invoke<Status>('install_latest');

export const StartService = () => invoke<Status>('start_service');

export const StopService = () => invoke<Status>('stop_service');

export const RestartService = () => invoke<Status>('restart_service');

export const ReadConfig = () => invoke<ConfigPayload>('read_config');

export const SaveConfig = (payload: ConfigPayload) =>
  invoke<ConfigPayload>('save_config', {payload});

export const CheckCoreUpdate = () => invoke<UpdateInfo>('check_core_update');

export const UpdateCore = () => invoke<Status>('update_core');

export const CheckAppUpdate = () => invoke<AppUpdateInfo>('check_app_update');

/** Resolves on macOS; on Windows the installer takes over and the app exits. */
export const InstallAppUpdate = () => invoke<void>('install_app_update');

export const ReadLogs = (limit: number) => invoke<string>('read_logs', {limit});

export const ClearLogs = () => invoke<Status>('clear_logs');

export const GetSettings = () => invoke<AppSettings>('get_settings');

export const SaveSettings = (settings: AppSettings) =>
  invoke<AppSettings>('save_settings', {settings});

/** Reveal the window. The backend starts it hidden so the first frame the user
 *  sees is the finished one, and the shell calls this once it has painted. */
export const ShowMainWindow = () => invoke<void>('show_main_window');

/** Quit for real. Closing the window only hides it. */
export const QuitApp = () => invoke<void>('quit_app');
