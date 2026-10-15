// モバイル版の API の入口。画面はここの `mobileApi` だけを呼ぶ。
// Tauri 実行時は実バックエンド、ブラウザ単体（pnpm dev / preview）ではモックを使う。
import { mobileMockApi } from './mobile-mock';
import { mobileTauriApi } from './mobile';
import type { MobileApi } from './mobile-types';

const isTauriRuntime = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const mobileApi: MobileApi = isTauriRuntime ? mobileTauriApi : mobileMockApi;
