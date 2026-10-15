// モバイル版の画面の状態（どの保存先を開いているか、どのタブ・フォルダ・プレビューか）。
// 端末内の設定として、最後に開いた保存先だけを localStorage に覚える（docs/design.md 14.7 M1）。

import type { MobileRepo } from '#lib/api/mobile-types.js';
import { resetDraft, syncUploadFolder } from './upload-draft.svelte';

export type MobileTab = 'files' | 'upload' | 'settings';

export interface RepoRef {
	owner: string;
	name: string;
}

/**
 * プレビューの対象。`sha` は一覧にある blob の sha（過去の版でも同じ）。
 * 過去の版のときは `versionRef` にそのコミットの sha を入れ、`versionLabel` に日付を入れる。現在の版は `versionRef` が null
 */
export interface PreviewTarget {
	path: string;
	name: string;
	sha: string;
	versionRef: string | null;
	versionLabel: string | null;
}

const LAST_REPO_KEY = 'hikae.mobile.lastRepo';

export function refOf(repo: MobileRepo | RepoRef): RepoRef {
	return { owner: repo.owner, name: repo.name };
}

function readLastRepo(): RepoRef | null {
	try {
		const raw = localStorage.getItem(LAST_REPO_KEY);
		if (!raw) return null;
		const value = JSON.parse(raw) as Partial<RepoRef>;
		if (typeof value.owner === 'string' && typeof value.name === 'string') {
			return { owner: value.owner, name: value.name };
		}
		return null;
	} catch {
		return null;
	}
}

function writeLastRepo(repo: RepoRef | null): void {
	try {
		if (repo) localStorage.setItem(LAST_REPO_KEY, JSON.stringify(repo));
		else localStorage.removeItem(LAST_REPO_KEY);
	} catch {
		return;
	}
}

interface NavState {
	/** 開いている保存先。null なら保存先の選択から始める */
	repo: RepoRef | null;
	/** 保存先を選び直している最中か（保存先が決まっていても一覧を出す） */
	choosingRepo: boolean;
	/** 認証が切れたなどで、ログインし直す画面を出すか */
	needsLogin: boolean;
	tab: MobileTab;
	/** ファイルの画面で開いているフォルダ（ルートは空文字） */
	folder: string;
	preview: PreviewTarget | null;
}

export const nav: NavState = $state({
	repo: readLastRepo(),
	choosingRepo: false,
	needsLogin: false,
	tab: 'files',
	folder: '',
	preview: null
});

export function chooseRepo(repo: RepoRef): void {
	nav.repo = refOf(repo);
	nav.choosingRepo = false;
	nav.tab = 'files';
	nav.folder = '';
	nav.preview = null;
	resetDraft('');
	writeLastRepo(nav.repo);
}

export function beginRepoChange(): void {
	nav.choosingRepo = true;
}

export function endRepoChange(): void {
	nav.choosingRepo = false;
}

export function openFolder(path: string): void {
	nav.folder = path;
	nav.preview = null;
}

export function openPreview(target: PreviewTarget): void {
	nav.preview = target;
}

export function closePreview(): void {
	nav.preview = null;
}

export function setTab(tab: MobileTab): void {
	nav.tab = tab;
	nav.preview = null;
	// アップロードの追加先は、項目がまだ無いときだけ、開いているフォルダに合わせる
	if (tab === 'upload') syncUploadFolder(nav.folder);
}

/** 保存先の選択やログインの状態を、起動時の既定に戻す（テスト用） */
export function resetNav(): void {
	nav.repo = null;
	nav.choosingRepo = false;
	nav.needsLogin = false;
	nav.tab = 'files';
	nav.folder = '';
	nav.preview = null;
	resetDraft('');
}
