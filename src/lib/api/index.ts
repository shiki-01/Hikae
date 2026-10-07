import { mockApi } from './mock';
import { tauriApi } from './tauri';
import type { ProjectApi } from './types';

/** Tauri 実行時は実バックエンド、ブラウザ単体（pnpm dev / preview）ではモックを使う */
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const api: ProjectApi = isTauri ? tauriApi : mockApi;
