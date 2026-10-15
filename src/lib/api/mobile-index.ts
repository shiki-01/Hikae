// モバイル版の API の入口。画面はここの `mobileApi` だけを呼ぶ。
// 現在はブラウザ単体用のモックを使う。Tauri のバインディングを生成したら、
// `mobile.ts`（Tauri の呼び出し）の実装に差し替える（docs/design.md 14.8。mobile.ts は未実装）。
import { mobileMockApi } from './mobile-mock';
import type { MobileApi } from './mobile-types';

export const mobileApi: MobileApi = mobileMockApi;
