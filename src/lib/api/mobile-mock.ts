// モバイル版（/m）のブラウザ単体用のモック。`pnpm dev` で /m を開いたときに使う（Tauri では使わない）。
// メモリ上のリポジトリを読み書きし、同名判定と「両方残す」「置き換える」「追加しない」を再現する。
//
// 画面確認用の localStorage（既存の hikae.mock.login と同じ書き方）:
// - 'hikae.mobile.session' : { loggedIn } ログインの状態
// - 'hikae.mock.login'     : denied / expired / setup でログインの結末を再現
// - 'hikae.mock.mobile'    : github-down（一覧・取得が E04）/ reauth（一覧が E01 で、ログインし直しになる）/
//                            forbidden（アップロードが E02）

import { t } from '#lib/i18n/index.js';
import { base64ToBytes, textToBase64 } from '#lib/features/mobile/base64.js';
import { joinRepoPath, parentPathOf } from '#lib/features/mobile/breadcrumb.js';
import { MAX_FILE_BYTES } from '#lib/features/mobile/limits.js';
import { extensionOf, mimeOf } from '#lib/features/mobile/mime.js';
import { uniqueName } from '#lib/features/mobile/names.js';
import { AppError } from './errors';
import type {
	DocSection,
	MobileApi,
	MobileDirEntry,
	MobileDirListing,
	MobilePreview,
	MobileRepo,
	MobileVersion,
	PlanItemInput,
	UploadItemInput,
	UploadPlan,
	UploadResult
} from './mobile-types';
import type { DeviceFlow, LoginOutcome } from './types';

const DAY = 24 * 60 * 60 * 1000;
const WAIT = 150;
const SESSION_KEY = 'hikae.mobile.session';

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

function readSession(): { loggedIn: boolean } {
	try {
		const raw = localStorage.getItem(SESSION_KEY);
		return raw
			? { loggedIn: (JSON.parse(raw) as { loggedIn?: boolean }).loggedIn === true }
			: {
					loggedIn: false
				};
	} catch {
		return { loggedIn: false };
	}
}

function writeSession(loggedIn: boolean): void {
	try {
		localStorage.setItem(SESSION_KEY, JSON.stringify({ loggedIn }));
	} catch {
		return;
	}
}

function scenario(key: string): string | null {
	try {
		return localStorage.getItem(key);
	} catch {
		return null;
	}
}

function loginScenario(): string | null {
	return scenario('hikae.mock.login');
}

/** モックのバックエンドのエラー（技術情報とコードは、実際のバックエンドの形に合わせる） */
function backendError(code: string, technical: string): AppError {
	return new AppError(
		'backend',
		technical,
		{},
		{
			code,
			params: {},
			whatHappened: t('mock.mobile.error.what'),
			dataIsSafe: t('login.error.data_is_safe'),
			nextAction: t('mock.mobile.error.next')
		}
	);
}

// --- メモリ上のリポジトリ ---

interface MockFile {
	/** base64 の中身 */
	bytes: string;
	size: number;
	sha: string;
	/** 新しい順 */
	versions: MobileVersion[];
}

interface MockTree {
	repo: MobileRepo;
	files: Map<string, MockFile>;
	/** 明示的に作ったフォルダ（ファイルの親は自動で含める） */
	dirs: Set<string>;
	headSha: string;
}

let counter = 0;
function nextSha(prefix: string): string {
	counter += 1;
	return `${prefix}-${String(counter).padStart(6, '0')}`;
}

function daysAgo(days: number): string {
	return new Date(Date.now() - days * DAY).toISOString();
}

// 最小の PDF（pdf.js が読めれば十分。xref が無い簡素なもの）
const MINI_PDF = [
	'%PDF-1.4',
	'1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj',
	'2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj',
	'3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]/Resources<<>>>>endobj',
	'trailer<</Root 1 0 R>>',
	'%%EOF'
].join('\n');

// 1x1 の PNG
const MINI_PNG =
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';

function textFile(text: string, versions: MobileVersion[] = []): MockFile {
	const bytes = textToBase64(text);
	return { bytes, size: base64ToBytes(bytes).length, sha: nextSha('blob'), versions };
}

function binaryFile(bytes: string, size: number): MockFile {
	return { bytes, size, sha: nextSha('blob'), versions: [] };
}

function version(message: string, days: number): MobileVersion {
	return {
		sha: nextSha('commit'),
		message,
		date: daysAgo(days),
		author: t('mock.mobile.author')
	};
}

function createTrees(): Map<string, MockTree> {
	const trees = new Map<string, MockTree>();

	const home: MobileRepo = {
		owner: t('mock.mobile.author'),
		name: t('mock.mobile.repo.family'),
		private: true,
		defaultBranch: 'main',
		pushedAt: daysAgo(1),
		description: t('mock.mobile.repo.family_description')
	};
	const homeFiles = new Map<string, MockFile>();
	const initial = t('mock.mobile.commit.initial');
	homeFiles.set('README.md', textFile(t('mock.mobile.body.readme'), [version(initial, 30)]));
	homeFiles.set(
		`${t('mock.mobile.dir.docs')}/${t('mock.mobile.file.minutes')}`,
		textFile(t('mock.mobile.body.minutes'), [
			version(initial, 20),
			version(t('mock.mobile.commit.note'), 2)
		])
	);
	homeFiles.set(
		`${t('mock.mobile.dir.docs')}/${t('mock.mobile.file.estimate')}`,
		binaryFile(textToBase64('estimate'), 4096)
	);
	homeFiles.set(
		`${t('mock.mobile.dir.docs')}/${t('mock.mobile.file.sheet')}`,
		binaryFile(textToBase64('sheet'), 8192)
	);
	homeFiles.set(
		`${t('mock.mobile.dir.slides')}/${t('mock.mobile.file.deck')}`,
		binaryFile(textToBase64('deck'), 16384)
	);
	homeFiles.set(
		`${t('mock.mobile.dir.photos')}/${t('mock.mobile.file.logo')}`,
		binaryFile(MINI_PNG, 68)
	);
	homeFiles.set(
		`${t('mock.mobile.dir.manual')}/${t('mock.mobile.file.manual')}`,
		binaryFile(textToBase64(MINI_PDF), 512)
	);
	homeFiles.set(
		`${t('mock.mobile.dir.archive')}/${t('mock.mobile.file.zip')}`,
		binaryFile(textToBase64('zip'), 2048)
	);
	homeFiles.set(
		`${t('mock.mobile.dir.archive')}/${t('mock.mobile.file.video')}`,
		binaryFile('', MAX_FILE_BYTES + 5 * 1024 * 1024)
	);
	trees.set(key(home), {
		repo: home,
		files: homeFiles,
		dirs: new Set([
			t('mock.mobile.dir.docs'),
			t('mock.mobile.dir.slides'),
			t('mock.mobile.dir.photos'),
			t('mock.mobile.dir.manual'),
			t('mock.mobile.dir.archive')
		]),
		headSha: nextSha('head')
	});

	const shared: MobileRepo = {
		owner: 'hikae-team',
		name: t('mock.mobile.repo.shared'),
		private: false,
		defaultBranch: 'main',
		pushedAt: daysAgo(12),
		description: null
	};
	trees.set(key(shared), {
		repo: shared,
		files: new Map([['README.md', textFile(t('mock.mobile.body.shared'), [version(initial, 12)])]]),
		dirs: new Set(),
		headSha: nextSha('head')
	});
	return trees;
}

function key(repo: { owner: string; name: string }): string {
	return `${repo.owner}/${repo.name}`;
}

let trees: Map<string, MockTree> | null = null;

function treeOf(owner: string, name: string): MockTree {
	trees ??= createTrees();
	const tree = trees.get(`${owner}/${name}`);
	if (!tree) throw new AppError('E11', `repo not found: ${owner}/${name}`, { name });
	return tree;
}

function lastSegment(path: string): string {
	return path.slice(path.lastIndexOf('/') + 1);
}

function previewOf(path: string, file: MockFile): MobilePreview {
	const ext = extensionOf(path);
	if (file.size > MAX_FILE_BYTES) return { kind: 'tooLarge' };
	if (['txt', 'md', 'csv', 'json'].includes(ext)) {
		return {
			kind: 'text',
			text: new TextDecoder().decode(base64ToBytes(file.bytes)),
			truncated: false
		};
	}
	if (['png', 'jpg', 'jpeg', 'gif', 'webp'].includes(ext)) {
		return { kind: 'image', mime: mimeOf(path), contentBase64: file.bytes };
	}
	if (ext === 'pdf') return { kind: 'pdf', contentBase64: file.bytes };
	if (ext === 'docx' || ext === 'xlsx' || ext === 'pptx') {
		return { kind: 'document', format: ext, sections: sectionsOf(ext) };
	}
	return { kind: 'unsupported' };
}

function sectionsOf(format: 'docx' | 'xlsx' | 'pptx'): DocSection[] {
	if (format === 'xlsx') {
		return [
			{
				title: t('mock.mobile.doc.sheet'),
				paragraphs: [],
				rows: [
					[t('mock.mobile.doc.item'), t('mock.mobile.doc.amount')],
					[t('mock.mobile.doc.food'), '120']
				]
			}
		];
	}
	const title = format === 'pptx' ? t('mock.mobile.doc.slide') : null;
	return [{ title, paragraphs: [t('mock.mobile.doc.paragraph')], rows: null }];
}

/** フォルダの直下の項目（ファイルと、子フォルダ） */
function entriesOf(tree: MockTree, folder: string): MobileDirEntry[] {
	const entries: MobileDirEntry[] = [];
	for (const [path, file] of tree.files) {
		if (parentPathOf(path) === folder) {
			entries.push({
				name: lastSegment(path),
				path,
				kind: 'file',
				size: file.size,
				sha: file.sha
			});
		}
	}
	const dirs = new Set<string>(tree.dirs);
	for (const path of tree.files.keys()) {
		let parent = parentPathOf(path);
		while (parent !== '') {
			dirs.add(parent);
			parent = parentPathOf(parent);
		}
	}
	for (const dir of dirs) {
		if (parentPathOf(dir) === folder && dir !== folder) {
			entries.push({ name: lastSegment(dir), path: dir, kind: 'dir', size: 0, sha: `tree-${dir}` });
		}
	}
	return entries;
}

function fileAt(tree: MockTree, path: string): MockFile | undefined {
	return tree.files.get(path);
}

export const mobileMockApi: MobileApi = {
	async getAccount() {
		await sleep(WAIT);
		return readSession().loggedIn ? { login: t('mock.mobile.author'), avatarUrl: null } : null;
	},

	async startLogin(): Promise<DeviceFlow> {
		await sleep(WAIT);
		if (loginScenario() === 'setup') {
			throw new AppError(
				'backend',
				'MissingClientId',
				{},
				{
					code: 'mock_login_setup',
					params: {},
					whatHappened: t('mock.login.setup_missing'),
					dataIsSafe: t('login.error.data_is_safe'),
					nextAction: t('login.error.setup.next')
				}
			);
		}
		return {
			userCode: 'WDJB-MJHT',
			verificationUri: 'https://github.com/login/device',
			expiresInSecs: 900
		};
	},

	async openLoginPage(): Promise<void> {
		// ブラウザ単体では開かない（画面はリンクを出さない）
	},

	async waitLogin(): Promise<LoginOutcome> {
		const outcome = loginScenario();
		await sleep(3000);
		if (outcome === 'denied' || outcome === 'expired') return outcome;
		writeSession(true);
		return 'succeeded';
	},

	async cancelLogin(): Promise<void> {
		// 待ちを止める処理はモックでは持たない（waitLogin は数秒で終わる）
	},

	async logout(): Promise<void> {
		await sleep(WAIT);
		writeSession(false);
	},

	async listRepos(): Promise<MobileRepo[]> {
		await sleep(WAIT);
		const mode = scenario('hikae.mock.mobile');
		if (mode === 'github-down') throw new AppError('E04', 'mock: github unavailable');
		if (mode === 'reauth') throw new AppError('E01', 'mock: token expired');
		trees ??= createTrees();
		return [...trees.values()].map((tree) => tree.repo);
	},

	async listDir(owner, name, path): Promise<MobileDirListing> {
		await sleep(WAIT);
		if (scenario('hikae.mock.mobile') === 'github-down') {
			throw new AppError('E04', 'mock: github unavailable');
		}
		const tree = treeOf(owner, name);
		return {
			branch: tree.repo.defaultBranch,
			headSha: tree.headSha,
			truncated: false,
			entries: entriesOf(tree, path)
		};
	},

	// `sha` と `ref`（過去の版のコミット）は見ない。モックは版ごとの内容を持たず、現在の内容を返す
	async preview(owner, name, path): Promise<MobilePreview> {
		await sleep(WAIT);
		const file = fileAt(treeOf(owner, name), path);
		if (!file) throw new AppError('E11', `file not found: ${path}`, { name: lastSegment(path) });
		return previewOf(path, file);
	},

	// `sha` と `ref` は preview と同じく見ない（現在の内容を返す）
	async download(owner, name, path) {
		await sleep(WAIT);
		const file = fileAt(treeOf(owner, name), path);
		if (!file) throw new AppError('E11', `file not found: ${path}`, { name: lastSegment(path) });
		if (file.size > MAX_FILE_BYTES) throw backendError('mobile_too_large', 'mock: too large');
		return { fileName: lastSegment(path), mime: mimeOf(path), contentBase64: file.bytes };
	},

	async fileHistory(owner, name, path): Promise<MobileVersion[]> {
		await sleep(WAIT);
		return [...(fileAt(treeOf(owner, name), path)?.versions ?? [])];
	},

	async planUpload(owner, name, items: PlanItemInput[]): Promise<UploadPlan> {
		await sleep(WAIT);
		const tree = treeOf(owner, name);
		return {
			branch: tree.repo.defaultBranch,
			items: items.map((item) => {
				const targetPath = joinRepoPath(item.targetDir, item.name);
				return { targetPath, exists: tree.files.has(targetPath) };
			})
		};
	},

	async upload(owner, name, message, items: UploadItemInput[]): Promise<UploadResult> {
		await sleep(WAIT * 2);
		if (scenario('hikae.mock.mobile') === 'forbidden') {
			throw new AppError('E02', 'mock: forbidden');
		}
		const tree = treeOf(owner, name);
		const text = message ?? t('mobile.upload.default_message', { n: items.length });
		const saved: string[] = [];
		const skipped: string[] = [];
		const renamed: { from: string; to: string }[] = [];
		const commitSha = nextSha('commit');
		for (const item of items) {
			let path = joinRepoPath(item.targetDir, item.name);
			const existing = tree.files.get(path);
			if (existing) {
				if (item.onExists === 'skip') {
					skipped.push(path);
					continue;
				}
				if (item.onExists === 'keepBoth') {
					const siblings = new Set(
						[...tree.files.keys()]
							.filter((file) => parentPathOf(file) === item.targetDir)
							.map(lastSegment)
					);
					const newName = uniqueName(item.name, siblings);
					const to = joinRepoPath(item.targetDir, newName);
					renamed.push({ from: path, to });
					path = to;
				}
			}
			const versions = [
				{
					sha: commitSha,
					message: text,
					date: new Date().toISOString(),
					author: t('mock.mobile.author')
				},
				...(tree.files.get(path)?.versions ?? [])
			];
			tree.files.set(path, {
				bytes: item.contentBase64,
				size: base64ToBytes(item.contentBase64).length,
				sha: nextSha('blob'),
				versions
			});
			saved.push(path);
		}
		if (saved.length > 0) tree.headSha = commitSha;
		return { commitSha: saved.length > 0 ? commitSha : '', saved, skipped, renamed };
	}
};
