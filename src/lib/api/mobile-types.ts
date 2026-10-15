// モバイル版（/m）の API の型（docs/design.md 14.8）。
// 将来は Rust 側の定義から生成する（生成先は bindings-mobile.ts）。それまではここを正とする。
import type { DeviceFlow, LoginOutcome } from './types';

/** ログイン中の GitHub のアカウント */
export interface MobileAccount {
	login: string;
	avatarUrl: string | null;
}

/** 保存先の候補（GitHub 上のリポジトリ） */
export interface MobileRepo {
	owner: string;
	name: string;
	private: boolean;
	defaultBranch: string;
	pushedAt: string | null;
	description: string | null;
}

/** フォルダの中の 1 項目。`path` はリポジトリのルートからのパス（区切りは `/`、ルートは空文字） */
export interface MobileDirEntry {
	name: string;
	path: string;
	kind: 'file' | 'dir';
	size: number;
	/** ファイルなら blob の sha。フォルダなら tree の sha */
	sha: string;
}

export interface MobileDirListing {
	branch: string;
	headSha: string;
	/** 1,000 件の上限を超えて一部のみ返したか */
	truncated: boolean;
	entries: MobileDirEntry[];
}

/** docx・xlsx・pptx の本文の 1 区切り（段落の塊、またはシート・スライド） */
export interface DocSection {
	title: string | null;
	paragraphs: string[];
	rows: string[][] | null;
}

/**
 * ファイルの表示用の内容。形式の判定は Rust 側で行う。
 * - `text`: UTF-8 のテキスト。長いときは `truncated`
 * - `image`: png・jpeg・gif・webp・svg
 * - `pdf`: PDF（表示は pdf.js で画面側が描く）
 * - `document`: docx・xlsx・pptx（本文を区切りごとに返す）
 * - `unsupported`: 表示できない形式、または文字コードを推定せずに表示できないもの
 * - `tooLarge`: 25 MiB を超える（取得していない）
 */
export type MobilePreview =
	| { kind: 'text'; text: string; truncated: boolean }
	| { kind: 'image'; mime: string; contentBase64: string }
	| { kind: 'pdf'; contentBase64: string }
	| { kind: 'document'; format: 'docx' | 'xlsx' | 'pptx'; sections: DocSection[] }
	| { kind: 'unsupported' }
	| { kind: 'tooLarge' };

/** 過去の版（そのファイルを変えたコミット）。`sha` はコミットの sha */
export interface MobileVersion {
	sha: string;
	message: string;
	date: string;
	author: string;
}

export interface MobileDownload {
	fileName: string;
	mime: string;
	contentBase64: string;
}

/** アップロードの計画の入力。`targetDir` は追加先のフォルダ（ルートは空文字） */
export interface PlanItemInput {
	targetDir: string;
	name: string;
	size: number;
}

export interface UploadPlan {
	branch: string;
	/** 入力と同じ順序。`exists` は同じパスのファイルが既にあるか */
	items: { targetPath: string; exists: boolean }[];
}

/** 同名のファイルがあったときの扱い。`keepBoth` は `名前 (2).拡張子` の形で残す */
export type OnExists = 'replace' | 'keepBoth' | 'skip';

export interface UploadItemInput {
	targetDir: string;
	name: string;
	contentBase64: string;
	onExists: OnExists;
}

export interface UploadResult {
	/** 1 件も保存しなかったときは空文字（コミットを作らない） */
	commitSha: string;
	/** 保存したファイルのパス */
	saved: string[];
	/** 追加しなかったファイルのパス */
	skipped: string[];
	/** 「両方残す」で名前を変えたもの（パス。リポジトリのルートから） */
	renamed: { from: string; to: string }[];
}

export interface MobileApi {
	getAccount(): Promise<MobileAccount | null>;
	startLogin(): Promise<DeviceFlow>;
	openLoginPage(): Promise<void>;
	waitLogin(): Promise<LoginOutcome>;
	cancelLogin(): Promise<void>;
	logout(): Promise<void>;
	listRepos(): Promise<MobileRepo[]>;
	listDir(owner: string, name: string, path: string, ref?: string): Promise<MobileDirListing>;
	/**
	 * `sha` は一覧のファイルの blob の sha。`ref`（過去の版のコミットの sha）を指定したときは、
	 * その時点のファイルを返し、`sha` は無視される
	 */
	preview(
		owner: string,
		name: string,
		path: string,
		sha: string,
		ref?: string
	): Promise<MobilePreview>;
	/** `sha` と `ref` の扱いは `preview` と同じ */
	download(
		owner: string,
		name: string,
		path: string,
		sha: string,
		ref?: string
	): Promise<MobileDownload>;
	fileHistory(owner: string, name: string, path: string): Promise<MobileVersion[]>;
	planUpload(owner: string, name: string, items: PlanItemInput[]): Promise<UploadPlan>;
	upload(
		owner: string,
		name: string,
		message: string | null,
		items: UploadItemInput[]
	): Promise<UploadResult>;
}
