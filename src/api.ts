import {invoke} from '@tauri-apps/api/core';

export type Status = {
  installed: boolean;
  running: boolean;
  loaded: boolean;
  version: string;
  mode: string;
  pid: number;
  logTail: string;
  adminReady: boolean;
  adminError: string;
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

export const emptyStatus: Status = {
  installed: false,
  running: false,
  loaded: false,
  version: '',
  mode: 'web',
  pid: 0,
  logTail: '',
  adminReady: false,
  adminError: '',
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
