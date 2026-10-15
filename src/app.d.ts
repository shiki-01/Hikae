// SvelteKit のグローバルな型の追加。モバイル版のアプリ（docs/design.md 14.3）が初期化スクリプトで設定するフラグ。
// デスクトップでは未設定（undefined）のまま。
interface Window {
	__HIKAE_MOBILE__?: boolean;
}
