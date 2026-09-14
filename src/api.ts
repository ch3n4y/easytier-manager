import {invoke} from '@tauri-apps/api/core';

export type Status = {
  installed: boolean;
  running: boolean;
  loaded: boolean;
  version: string;
  mode: string;
  pid: number;
  adminReady: boolean;
  adminError: string;
  /** Host platform, used to tailor the authorization copy. */
  platform: string;
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

export type WindowMode = 'compact' | 'expanded';

export type WindowModeReport = {
  mode: WindowMode;
  width: number;
  height: number;
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

export const ReadLogs = (limit: number) => invoke<string>('read_logs', {limit});

export const ClearLogs = () => invoke<Status>('clear_logs');

export const GetSettings = () => invoke<AppSettings>('get_settings');

export const SaveSettings = (settings: AppSettings) =>
  invoke<AppSettings>('save_settings', {settings});

export const SetWindowMode = (mode: WindowMode) =>
  invoke<WindowModeReport>('set_window_mode', {mode});
