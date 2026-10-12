import { t } from '#lib/i18n/index.js';
import { AppError } from './errors';
import { suggestMemo } from '#lib/features/changes/memo.js';
import { diffLines } from './diff-lines';
import { officeKindOf } from '#lib/features/compare/office-diff.js';
import { classifyDropped, LARGE_WARN_BYTES, MAX_FILE_BYTES } from '#lib/features/changes/files.js';
import type {
	InterruptedRecovery,
	OfficeDiff,
	AiCatalog,
	AiCustomGgufCheck,
	AiModel,
	AiDownloadNotice,
	AiEndpointCheck,
	AiModelStatus,
	AiTestResult,
	AiSuggestion,
	SensitiveKind,
	FilePreview,
	PreviewSheet,
	PreviewSlide,
	RendererInfo,
	AddFilesOptions,
	AddFilesOutcome,
	AddProjectInput,
	AddProjectResult,
	AfterRestoreSave,
	AppSettings,
	AppHealth,
	FolderCheck,
	FolderWarning,
	Change,
	ClonePhase,
	ConflictFile,
	ConflictResolution,
	ConnectRemoteInput,
	DeviceFlow,
	DiscardResult,
	DroppedFile,
	FetchOptions,
	FetchResult,
	FileDiff,
	FileEntry,
	ImpactItem,
	LoginOutcome,
	Owner,
	PointFile,
	Project,
	ProjectApi,
	ProjectFile,
	ProjectTree,
	PushResult,
	RemoteProject,
	RemoteOutcome,
	RemoteProjectList,
	IgnoreKind,
	IgnoreRule,
	SkipWorktreeEntry,
	SkipStashEntry,
	SkipStashExtracted,
	SkipStashList,
	RestoreResult,
	RestoreScope,
	SaveOutcome,
	SavePoint,
	Session,
	SettingKey,
	SettingsView,
	SizeCheck,
	SizeChoice
} from './types';

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const PSD_BYTES = 72 * 1024 * 1024;
const VIDEO_BYTES = 250 * 1024 * 1024;
const WAIT_SHORT = 350;
const WAIT_LONG = 900;

interface Snapshot {
	changes: Change[];
	savePoints: SavePoint[];
	tiers: Map<string, number>;
	currentTier: number;
	project: Project;
}

interface ProjectState extends Snapshot {
	conflicts: ConflictFile[];
	conflictsOnNextFetch: boolean;
	pushFailuresLeft: number;
	restoreFailuresLeft: number;
}

/**
 * 保存前の検査で使う、モックのファイルサイズ。
 * .psd は 72MB（警告）、.mp4 は 250MB（保存できない）として扱い、画面の確認を開発中に試せるようにする。
 */
function mockSizeOf(path: string): number {
	if (path.endsWith('.psd')) return PSD_BYTES;
	if (path.endsWith('.mp4')) return VIDEO_BYTES;
	return 12 * 1024;
}

function inspectSizes(changes: Change[]): SizeCheck {
	const sized = changes
		.filter((c) => c.type !== 'deleted')
		.map((c) => ({ path: c.path, sizeBytes: mockSizeOf(c.path) }));
	return {
		blocked: sized.filter((f) => f.sizeBytes >= MAX_FILE_BYTES),
		warned: sized.filter((f) => f.sizeBytes >= LARGE_WARN_BYTES && f.sizeBytes < MAX_FILE_BYTES)
	};
}

/** 大きいファイルの選択を反映して保存する。まだ確認が必要なファイルが残れば、何も保存せず確認を返す */
function saveWithCheck(state: ProjectState, memo: string, choice?: SizeChoice): SaveOutcome {
	if (choice) {
		const excluded = new Set(choice.exclude);
		state.changes = state.changes.filter((c) => !excluded.has(c.path));
		state.project.unsavedCount = state.changes.length;
	}
	const check = inspectSizes(state.changes);
	const warned = choice?.acceptWarned ? [] : check.warned;
	if (check.blocked.length > 0 || warned.length > 0) {
		return { kind: 'size_check', check: { blocked: check.blocked, warned } };
	}
	if (state.changes.length > 0) addSavePoint(state, memo, 3);
	return { kind: 'saved' };
}

/**
 * 保存先の作成で試せる場面（名前で切り替える）。
 * - `existing-empty`: 同じ名前の空の保存先がすでにある（確認のあと、承認すれば接続できる）
 * - `taken`: 同じ名前の空でない保存先がある（接続できない）
 * - 追加時の名前に `large` を含める: 最初の保存に大きいファイルがあり、作成の前に確認が返る
 */
export const MOCK_EXISTING_EMPTY = 'existing-empty';
export const MOCK_TAKEN = 'taken';

function mockNameTaken(): AppError {
	return new AppError(
		'backend',
		'name taken',
		{},
		{
			code: 'remote_name_taken',
			params: {},
			whatHappened: t('errcode.remote_name_taken.what'),
			dataIsSafe: t('errcode.remote_name_taken.safe'),
			nextAction: t('errcode.remote_name_taken.next')
		}
	);
}

/** 保存先の名前から、作成の前に起きる場面を決める。何も起きなければ null（そのまま作成して接続する） */
function mockRemoteCheck(ownerId: string, name: string, adopt: boolean): RemoteOutcome | null {
	if (name === MOCK_TAKEN) throw mockNameTaken();
	if (name === MOCK_EXISTING_EMPTY) {
		if (adopt) return null;
		return {
			repository: null,
			connected: false,
			uploaded: false,
			sizeCheck: null,
			existingEmptyRepository: `${ownerId}/${name}`,
			error: null
		};
	}
	// 空でない（または存在しない）保存先には、承認されても接続しない
	if (adopt) throw mockNameTaken();
	return null;
}

/** モックのプレビューの xlsx。今の版（current）には Sheet2 が無く Sheet3 があり、Sheet1 の値が変わっている */
function mockSheets(now: boolean): PreviewSheet[] {
	const cells: PreviewSheet['cells'] = [
		{ row: 0, col: 0, value: t('mock.xlsx.item') },
		{ row: 0, col: 1, value: t('mock.xlsx.value') },
		{ row: 0, col: 2, value: t('mock.xlsx.note') },
		{ row: 1, col: 0, value: t('mock.xlsx.item') },
		{ row: 1, col: 1, value: '100' },
		{ row: 2, col: 0, value: t('mock.xlsx.item') },
		{ row: 2, col: 1, value: now ? '150' : '120' }
	];
	if (now) cells.push({ row: 4, col: 2, value: t('mock.office.value') });
	const main: PreviewSheet = { name: 'Sheet1', rows: 5, cols: 3, cells, truncated: false };
	if (now) {
		return [
			main,
			{
				name: 'Sheet3',
				rows: 2,
				cols: 2,
				cells: [{ row: 0, col: 0, value: t('mock.xlsx.added') }],
				truncated: false
			}
		];
	}
	return [
		main,
		{
			name: 'Sheet2',
			rows: 1,
			cols: 1,
			cells: [{ row: 0, col: 0, value: t('mock.xlsx.dropped') }],
			truncated: false
		}
	];
}

/** モックのプレビューのスライド（pptx） */
function mockSlides(): PreviewSlide[] {
	return [
		{ index: 1, paragraphs: [t('mock.pptx.title'), t('mock.pptx.point1'), t('mock.pptx.point2')] },
		{ index: 2, paragraphs: [t('mock.pptx.result'), t('mock.pptx.result1')] },
		{ index: 3, paragraphs: [t('mock.pptx.next'), t('mock.pptx.next1')] }
	];
}

/** モックで「内容を表示できない」理由にする拡張子（実際の判定はバックエンドが行う） */
const MOCK_BINARY = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'pdf', 'psd'];
const MOCK_UNSUPPORTED = ['zip'];
const MOCK_CORRUPT = ['rar'];
const MOCK_TOO_LARGE = ['tar'];

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

function ago(ms: number): Date {
	return new Date(Date.now() - ms);
}

function lines(path: string, tier: number): string[] | null {
	if (path === t('mock.file.chapter3')) {
		const experiments = tier >= 4 ? 6 : tier >= 3 ? 5 : tier >= 2 ? 4 : 3;
		const result = [
			t('mock.doc.title'),
			t('mock.doc.prior'),
			t('mock.doc.experiment', { n: experiments }),
			t('mock.doc.conclusion')
		];
		if (tier >= 1) result.push(t('mock.doc.reference'));
		if (tier >= 3) result.push(t('mock.doc.extra'));
		return result;
	}
	if (path === t('mock.file.old_draft')) {
		return tier <= 2 ? [t('mock.draft.line1'), t('mock.draft.line2')] : null;
	}
	if (path === t('mock.file.memo')) {
		return tier >= 4
			? [t('mock.memo_file.line1'), t('mock.memo_file.cloud')]
			: [t('mock.memo_file.line1'), t('mock.memo_file.line2')];
	}
	if (path === t('mock.file.refs')) {
		return [t('mock.refs.line1'), t('mock.refs.line2')];
	}
	return null;
}

function createThesis(): ProjectState {
	const chapter = t('mock.file.chapter3');
	return {
		project: {
			id: 'proj1',
			name: t('mock.project.thesis'),
			path: 'C:\\Users\\student\\Documents\\Thesis',
			ownerName: 'shiki-01',
			ownerKind: 'personal',
			lastSavedAt: ago(HOUR),
			remoteConnected: true,
			lastUploadedAt: ago(DAY),
			unsavedCount: 5,
			uploadPendingCount: 1,
			fetchPendingCount: 2,
			hasConflict: false,
			folderMissing: false,
			repoBroken: false,
			repoLarge: false,
			interruptedOperation: null,
			watching: false,
			lastAutoSnapshotAt: ago(7 * MINUTE)
		},
		savePoints: [
			{
				id: 'sp1',
				createdAt: ago(HOUR),
				message: t('mock.memo.s1'),
				kind: 'save',
				cloudSynced: false,
				pcName: t('mock.pc.laptop')
			},
			{ id: 'sp2', createdAt: ago(75 * MINUTE), message: '', kind: 'auto', cloudSynced: false },
			{ id: 'sp3', createdAt: ago(100 * MINUTE), message: '', kind: 'auto', cloudSynced: false },
			{
				id: 'sp4',
				createdAt: ago(DAY),
				message: t('mock.memo.s4'),
				kind: 'save',
				cloudSynced: true,
				pcName: t('mock.pc.desktop')
			},
			{
				id: 'sp5',
				createdAt: ago(3 * DAY),
				message: t('mock.memo.s5'),
				kind: 'save',
				cloudSynced: true,
				pcName: t('mock.pc.desktop')
			}
		],
		tiers: new Map([
			['sp1', 2],
			['sp2', 2],
			['sp3', 2],
			['sp4', 1],
			['sp5', 0]
		]),
		currentTier: 3,
		changes: [
			{ id: 'c1', path: chapter, type: 'modified', isConflict: false },
			{ id: 'c2', path: t('mock.file.fig4'), type: 'added', isConflict: false, untracked: true },
			{ id: 'c3', path: t('mock.file.old_draft'), type: 'deleted', isConflict: false },
			{ id: 'c4', path: t('mock.file.logo'), type: 'added', isConflict: false, untracked: true },
			{
				id: 'c5',
				path: t('mock.file.draft_new'),
				type: 'added',
				isConflict: false,
				untracked: true
			}
		],
		conflicts: [
			{
				path: chapter,
				kind: 'both',
				thisPcSavedAt: ago(14 * HOUR),
				cloudSavedAt: ago(17 * HOUR),
				thisPcName: t('mock.pc.desktop'),
				cloudPcName: t('mock.pc.laptop')
			},
			{
				path: t('mock.file.memo'),
				kind: 'deleted_in_cloud',
				thisPcSavedAt: ago(15 * HOUR),
				cloudSavedAt: ago(16 * HOUR),
				thisPcName: null,
				cloudPcName: t('mock.pc.laptop')
			}
		],
		conflictsOnNextFetch: true,
		pushFailuresLeft: 1,
		restoreFailuresLeft: 1
	};
}

function createMaterials(): ProjectState {
	return {
		project: {
			id: 'proj2',
			name: t('mock.project.materials'),
			path: 'C:\\Users\\student\\Documents\\Materials',
			ownerName: 'hikae-app',
			ownerKind: 'org',
			lastSavedAt: ago(2 * DAY),
			remoteConnected: true,
			lastUploadedAt: ago(5 * DAY),
			unsavedCount: 0,
			uploadPendingCount: 1,
			fetchPendingCount: 0,
			hasConflict: false,
			folderMissing: false,
			repoBroken: false,
			repoLarge: false,
			interruptedOperation: 'pull',
			watching: false,
			lastAutoSnapshotAt: null
		},
		changes: [],
		savePoints: [
			{
				id: 'm1',
				createdAt: ago(2 * DAY),
				message: t('mock.memo.s5'),
				kind: 'save',
				cloudSynced: false,
				pcName: t('mock.pc.desktop')
			}
		],
		tiers: new Map([['m1', 3]]),
		currentTier: 3,
		conflicts: [],
		conflictsOnNextFetch: false,
		pushFailuresLeft: 0,
		restoreFailuresLeft: 0
	};
}

function createAlbum(): ProjectState {
	return {
		project: {
			id: 'proj3',
			name: t('mock.project.album'),
			path: 'D:\\Photos\\Album',
			ownerName: 'shiki-01',
			ownerKind: 'personal',
			lastSavedAt: ago(9 * DAY),
			remoteConnected: true,
			lastUploadedAt: ago(9 * DAY),
			unsavedCount: 0,
			uploadPendingCount: 0,
			fetchPendingCount: 0,
			hasConflict: false,
			folderMissing: true,
			repoBroken: false,
			repoLarge: false,
			interruptedOperation: null,
			watching: false,
			lastAutoSnapshotAt: null
		},
		changes: [],
		savePoints: [],
		tiers: new Map(),
		currentTier: 3,
		conflicts: [],
		conflictsOnNextFetch: false,
		pushFailuresLeft: 0,
		restoreFailuresLeft: 0
	};
}

/** GitHub に接続していない、ローカルだけのプロジェクト（接続の流れを試すため） */
function createLocalNotes(): ProjectState {
	return {
		project: {
			id: 'proj4',
			name: t('mock.project.local_notes'),
			path: 'C:\\Users\\student\\Documents\\Notes',
			ownerName: 'shiki-01',
			ownerKind: 'personal',
			lastSavedAt: ago(3 * HOUR),
			remoteConnected: false,
			lastUploadedAt: null,
			unsavedCount: 0,
			uploadPendingCount: 0,
			fetchPendingCount: 0,
			hasConflict: false,
			folderMissing: false,
			repoBroken: false,
			repoLarge: false,
			interruptedOperation: null,
			watching: false,
			lastAutoSnapshotAt: null
		},
		changes: [],
		savePoints: [
			{
				id: 'n1',
				createdAt: ago(3 * HOUR),
				message: t('mock.memo.s5'),
				kind: 'save',
				cloudSynced: false
			}
		],
		tiers: new Map([['n1', 3]]),
		currentTier: 3,
		conflicts: [],
		conflictsOnNextFetch: false,
		pushFailuresLeft: 0,
		restoreFailuresLeft: 0
	};
}

const states = new Map<string, ProjectState>();
for (const state of [createThesis(), createMaterials(), createAlbum(), createLocalNotes()]) {
	states.set(state.project.id, state);
}
const undoSnapshots = new Map<string, Snapshot>();
let sequence = 100;

const DEFAULT_SETTINGS: AppSettings = {
	pullOnStartup: true,
	pullIntervalMinutes: 15,
	saveBeforePull: true,
	conflictMode: 'notify-only',
	autoPushAfterSave: true,
	pushReminderHours: 24,
	autoSnapshotEnabled: true,
	autoSnapshotDelaySecs: 120,
	snapshotRetentionDays: 90,
	snapshotSizeCapX: 2,
	autoSaveAfterRestore: false,
	largeFileWarnMb: 50,
	showSnapshotsInTimeline: 'collapsed',
	memoSuggestion: 'rules',
	aiRunner: 'builtin',
	aiModel: '',
	aiModelSource: 'catalog',
	aiCustomGgufPath: '',
	aiScope: 'text-diff',
	aiSuggestBody: true,
	aiOllamaUrl: 'http://localhost:11434'
};

// 画面確認用: localStorage の 'hikae.mock.login' に denied / expired / setup を入れると、ログインの結末を再現できる
function loginScenario(): string | null {
	try {
		return localStorage.getItem('hikae.mock.login');
	} catch {
		return null;
	}
}

// 画面確認用: localStorage の 'hikae.mock.reauth' に 1 を入れると、トークンが切れた状態（再ログインが必要）を再現できる。
// ログインし直すと消える
function reauthScenario(): boolean {
	try {
		return localStorage.getItem('hikae.mock.reauth') === '1';
	} catch {
		return false;
	}
}

function clearReauthScenario(): void {
	try {
		localStorage.removeItem('hikae.mock.reauth');
	} catch {
		return;
	}
}

let cancelLoginWait: (() => void) | null = null;

function readStorage<T extends object>(key: string, fallback: T): T {
	try {
		const raw = localStorage.getItem(key);
		return raw ? { ...fallback, ...(JSON.parse(raw) as Partial<T>) } : fallback;
	} catch {
		return fallback;
	}
}

function writeStorage(key: string, value: unknown): void {
	try {
		localStorage.setItem(key, JSON.stringify(value));
	} catch {
		return;
	}
}

function readOverrides(): Record<string, Partial<AppSettings>> {
	return readStorage<Record<string, Partial<AppSettings>>>('hikae.settings.projects', {});
}

function stateOf(projectId: string): ProjectState {
	const state = states.get(projectId);
	if (!state) throw new AppError('E11', `project not found: ${projectId}`, { name: projectId });
	return state;
}

function tierOf(state: ProjectState, pointId: string): number {
	if (pointId === 'current') return state.currentTier;
	if (pointId === 'this') return 3;
	if (pointId === 'cloud') return 4;
	return state.tiers.get(pointId) ?? 0;
}

function nextId(prefix: string): string {
	sequence += 1;
	return `${prefix}${sequence}`;
}

function snapshotOf(state: ProjectState): Snapshot {
	return {
		changes: state.changes.map((c) => ({ ...c })),
		savePoints: state.savePoints.map((s) => ({ ...s })),
		tiers: new Map(state.tiers),
		currentTier: state.currentTier,
		project: { ...state.project }
	};
}

function addSavePoint(state: ProjectState, message: string, tier: number): SavePoint {
	const point: SavePoint = {
		id: nextId('sp'),
		createdAt: new Date(),
		message,
		kind: 'save',
		cloudSynced: false,
		pcName: t('mock.pc.desktop')
	};
	state.savePoints.unshift(point);
	state.tiers.set(point.id, tier);
	state.currentTier = tier;
	state.changes = [];
	state.project.unsavedCount = 0;
	state.project.lastSavedAt = point.createdAt;
	state.project.uploadPendingCount += 1;
	return point;
}

function impactFor(state: ProjectState, targetId: string, scope: RestoreScope): ImpactItem[] {
	const targetTier = tierOf(state, targetId);
	const paths = [
		t('mock.file.chapter3'),
		t('mock.file.old_draft'),
		t('mock.file.fig4'),
		t('mock.file.logo'),
		t('mock.file.refs')
	];
	const items: ImpactItem[] = [];
	for (const path of paths) {
		if (scope.kind === 'file' && scope.path !== path) continue;
		const change = state.changes.find((c) => c.path === path);
		if (change?.type === 'added') {
			items.push({ path, type: 'removed' });
			continue;
		}
		if (change?.type === 'deleted') {
			items.push({ path, type: 'restored' });
			continue;
		}
		const before = lines(path, state.currentTier);
		const after = lines(path, targetTier);
		if (before && !after) items.push({ path, type: 'removed' });
		else if (!before && after) items.push({ path, type: 'restored' });
		else if (before && after && before.join('\n') !== after.join('\n')) {
			items.push({ path, type: 'modified' });
		}
	}
	return items;
}

/** 保存しないファイルのパターン（プロジェクトごと。ブラウザ単体の確認用に保持する） */
const ignorePatterns = new Map<string, string[]>();

/** 退避に残っているファイル（プロジェクトごと。ブラウザ単体の確認用に保持する。初めて開いたときに 2 件入れる） */
const skipStashes = new Map<string, SkipStashEntry[]>();

function skipStashesOf(projectId: string): SkipStashEntry[] {
	let list = skipStashes.get(projectId);
	if (!list) {
		const now = Date.now() / 1000;
		list = [
			{
				id: 'mock-pending-1',
				path: 'デザイン/ロゴ.psd',
				sizeBytes: 12_400_000,
				savedAt: now - 3600,
				reason: 'pending'
			},
			{
				id: 'mock-deleted-1',
				path: '資料/旧版.docx',
				sizeBytes: 84_000,
				savedAt: now - 86_400,
				reason: 'deleted-upstream'
			}
		];
		skipStashes.set(projectId, list);
	}
	return list;
}

/** 「今後の変更だけ無視する」の記録（プロジェクトごと。ブラウザ単体の確認用に保持する） */
const skipRecords = new Map<string, { path: string; setAt: string; dismissed: boolean }[]>();

function skipRecordsOf(projectId: string) {
	return skipRecords.get(projectId) ?? [];
}

/** 記録を足す（同じパスは時刻を更新し、提案の印を戻す） */
function setMockSkipRecords(projectId: string, paths: string[]): void {
	const now = new Date().toISOString();
	const kept = skipRecordsOf(projectId).filter((r) => !paths.includes(r.path));
	skipRecords.set(projectId, [
		...kept,
		...paths.map((path) => ({ path, setAt: now, dismissed: false }))
	]);
}

function ignorePatternsOf(projectId: string): string[] {
	return ignorePatterns.get(projectId) ?? [];
}

/** バックエンドと同じ形の書き方を作る（エスケープは省く。確認用のため） */
function mockIgnorePattern(kind: IgnoreKind, value: string): string {
	const input = value.trim();
	if (kind === 'extension') return `*.${input.replace(/^\.+/, '')}`;
	if (kind === 'folder') return `${input.replace(/\\/g, '/').replace(/^\/+|\/+$/g, '')}/`;
	return `/${input.replace(/\\/g, '/').replace(/^\/+/, '')}`;
}

export const mockApi: ProjectApi = {
	capabilities: { restoreFile: true },

	async getSession(): Promise<Session> {
		await sleep(80);
		const stored = readStorage('hikae.session', { loggedIn: false, onboarded: false });
		return {
			loggedIn: stored.loggedIn,
			onboarded: stored.onboarded,
			reauthRequired: stored.loggedIn && reauthScenario(),
			userLogin: stored.loggedIn ? 'shiki-01' : null,
			avatarUrl: null
		};
	},

	async startLogin(): Promise<DeviceFlow> {
		await sleep(WAIT_SHORT);
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

	async waitLogin(): Promise<LoginOutcome> {
		const scenario = loginScenario();
		const canceled = await new Promise<boolean>((done) => {
			const timer = setTimeout(() => done(false), 3000);
			cancelLoginWait = () => {
				clearTimeout(timer);
				done(true);
			};
		});
		cancelLoginWait = null;
		if (canceled) return 'canceled';
		if (scenario === 'denied' || scenario === 'expired') return scenario;
		// ログインし直したときも、初回設定を終えたかは変えない
		const stored = readStorage('hikae.session', { loggedIn: false, onboarded: false });
		writeStorage('hikae.session', { loggedIn: true, onboarded: stored.onboarded });
		clearReauthScenario();
		return 'succeeded';
	},

	async cancelLogin(): Promise<void> {
		cancelLoginWait?.();
	},

	async logout(): Promise<void> {
		await sleep(120);
		const stored = readStorage('hikae.session', { loggedIn: false, onboarded: false });
		writeStorage('hikae.session', { loggedIn: false, onboarded: stored.onboarded });
		clearReauthScenario();
	},

	async openLoginPage(): Promise<void> {
		// ブラウザ単体では <a> が開くため、ここでは何もしない
	},

	async completeOnboarding(): Promise<void> {
		writeStorage('hikae.session', { loggedIn: true, onboarded: true });
	},

	async listProjects(): Promise<Project[]> {
		await sleep(WAIT_SHORT);
		return [...states.values()].map((s) => ({ ...s.project }));
	},

	async getProject(id: string): Promise<Project> {
		await sleep(120);
		return { ...stateOf(id).project };
	},

	async listOwners(): Promise<Owner[]> {
		await sleep(150);
		return [
			{ id: 'shiki-01', name: 'shiki-01', kind: 'personal', canCreate: true },
			{ id: 'hikae-app', name: 'hikae-app', kind: 'org', canCreate: true },
			{ id: 'campus-lab', name: 'campus-lab', kind: 'org', canCreate: false }
		];
	},

	async listRemoteProjects(query: string): Promise<RemoteProjectList> {
		await sleep(WAIT_SHORT);
		const all: RemoteProject[] = [
			{
				id: 'shiki-01/research',
				name: t('mock.remote.research'),
				ownerId: 'shiki-01',
				isPrivate: true
			},
			{
				id: 'shiki-01/recipes',
				name: t('mock.remote.recipes'),
				ownerId: 'shiki-01',
				isPrivate: true
			},
			{
				id: 'hikae-app/novel',
				name: t('mock.remote.novel'),
				ownerId: 'hikae-app',
				isPrivate: false
			},
			{
				id: 'campus-lab/minutes',
				name: t('mock.remote.minutes'),
				ownerId: 'campus-lab',
				isPrivate: true
			}
		];
		const needle = query.trim().toLowerCase();
		return {
			projects: needle ? all.filter((r) => r.name.toLowerCase().includes(needle)) : all,
			truncated: false
		};
	},

	async pickFiles(): Promise<string[] | null> {
		return null;
	},

	async pickFolder(): Promise<string | null> {
		await sleep(150);
		return 'C:\\Users\\student\\Documents\\NewFolder';
	},

	async pickGgufFile(): Promise<string | null> {
		await sleep(150);
		return 'C:\\Users\\student\\Models\\my-model.gguf';
	},

	async addProject(
		input: AddProjectInput,
		onProgress?: (phase: ClonePhase) => void
	): Promise<AddProjectResult> {
		if (input.mode === 'github') {
			for (const phase of ['preparing', 'downloading', 'finishing'] as const) {
				onProgress?.(phase);
				await sleep(WAIT_LONG / 2);
			}
			onProgress?.('done');
		} else {
			await sleep(WAIT_LONG);
		}
		const id = nextId('proj');
		const owners = await mockApi.listOwners();
		const owner = owners.find((o) => o.id === input.ownerId) ?? owners[0];
		const wantsCloud = input.mode !== 'github' && input.connectCloud === true;
		// 最初の保存に大きいファイルがあると、作成の前に確認を返す（何も作らず、接続もしない）
		const largeFirst = wantsCloud && input.name.includes('large');
		let early: RemoteOutcome | null = null;
		if (wantsCloud && !largeFirst) {
			try {
				early = mockRemoteCheck(owner.id, input.name, false);
			} catch (error) {
				// 保存先を作れなかった場合も、ローカルの登録は残る
				if (!(error instanceof AppError)) throw error;
				early = {
					repository: null,
					connected: false,
					uploaded: false,
					sizeCheck: null,
					existingEmptyRepository: null,
					error
				};
			}
		}
		const connected = input.mode === 'github' || (wantsCloud && !largeFirst && early === null);
		const project: Project = {
			id,
			name: input.name,
			path: input.mode === 'existing' ? input.folder : `${input.folder}\\${input.name}`,
			ownerName: owner.name,
			ownerKind: owner.kind,
			lastSavedAt: new Date(),
			remoteConnected: connected,
			lastUploadedAt: connected ? new Date() : null,
			unsavedCount: 0,
			uploadPendingCount: 0,
			fetchPendingCount: 0,
			hasConflict: false,
			folderMissing: false,
			repoBroken: false,
			repoLarge: false,
			interruptedOperation: null,
			watching: false,
			lastAutoSnapshotAt: null
		};
		states.set(id, {
			project,
			changes: largeFirst
				? [
						{
							id: nextId('c'),
							path: t('mock.file.logo'),
							type: 'added',
							isConflict: false,
							untracked: true
						}
					]
				: [],
			savePoints: [
				{
					id: nextId('sp'),
					createdAt: new Date(),
					message: t('mock.memo.s5'),
					kind: 'save',
					cloudSynced: connected
				}
			],
			tiers: new Map(),
			currentTier: 3,
			conflicts: [],
			conflictsOnNextFetch: false,
			pushFailuresLeft: 0,
			restoreFailuresLeft: 0
		});
		if (largeFirst) {
			project.unsavedCount = 1;
		}
		const remote: RemoteOutcome | null = !wantsCloud
			? null
			: largeFirst
				? {
						repository: null,
						connected: false,
						uploaded: false,
						sizeCheck: inspectSizes(states.get(id)?.changes ?? []),
						existingEmptyRepository: null,
						error: null
					}
				: (early ?? {
						repository: `${owner.id}/${input.name}`,
						connected: true,
						uploaded: true,
						sizeCheck: null,
						existingEmptyRepository: null,
						error: null
					});
		return { project: { ...project }, remote };
	},

	async connectRemote(id: string, input: ConnectRemoteInput): Promise<RemoteOutcome> {
		await sleep(WAIT_LONG);
		const state = stateOf(id);
		const owners = await mockApi.listOwners();
		const owner = owners.find((o) => o.id === input.ownerId && o.canCreate);
		if (!owner) throw new AppError('E02', 'owner cannot create');
		if (input.visibility === 'public' && !input.publicConfirmed) {
			throw new AppError('E02', 'public is not confirmed');
		}
		const repoName = input.name?.trim() || state.project.name;
		const early = mockRemoteCheck(owner.id, repoName, input.adoptExisting === true);
		if (early) return early;
		state.project.remoteConnected = true;
		state.project.ownerName = owner.name;
		state.project.ownerKind = owner.kind;
		state.project.uploadPendingCount = 0;
		state.project.lastUploadedAt = new Date();
		state.savePoints = state.savePoints.map((s) => ({ ...s, cloudSynced: s.kind === 'save' }));
		return {
			repository: `${owner.id}/${repoName}`,
			connected: true,
			uploaded: true,
			sizeCheck: null,
			existingEmptyRepository: null,
			error: null
		};
	},

	async removeProject(id: string): Promise<void> {
		await sleep(WAIT_SHORT);
		states.delete(id);
	},

	async relocateProject(id: string, folder: string): Promise<Project> {
		await sleep(WAIT_SHORT);
		const state = stateOf(id);
		state.project.path = folder;
		state.project.folderMissing = false;
		return { ...state.project };
	},

	async listChanges(projectId: string): Promise<Change[]> {
		await sleep(WAIT_SHORT);
		return stateOf(projectId).changes.map((c) => ({ ...c }));
	},

	async listSavePoints(projectId: string, offset: number, limit: number): Promise<SavePoint[]> {
		await sleep(WAIT_SHORT);
		return stateOf(projectId)
			.savePoints.slice(offset, offset + limit)
			.map((s) => ({ ...s }));
	},

	async listPointFiles(projectId: string, savePointId: string): Promise<PointFile[]> {
		await sleep(200);
		const state = stateOf(projectId);
		const tier = tierOf(state, savePointId);
		const paths = [t('mock.file.chapter3'), t('mock.file.old_draft'), t('mock.file.refs')];
		return paths
			.filter((path) => lines(path, tier) !== null)
			.map((path) => ({ path, type: 'modified' as const }));
	},

	async listFilesAt(projectId: string, savePointId: string): Promise<FileEntry[]> {
		await sleep(200);
		const tier = tierOf(stateOf(projectId), savePointId);
		const files: FileEntry[] = [
			t('mock.file.chapter3'),
			t('mock.file.old_draft'),
			t('mock.file.memo'),
			t('mock.file.refs')
		]
			.filter((path) => lines(path, tier) !== null)
			.map((path) => ({ path, size: 24_576 }));
		if (tier >= 1) files.push({ path: t('mock.file.fig4'), size: 184_320 });
		return files;
	},

	async suggestMemo(projectId: string): Promise<string> {
		await sleep(120);
		return suggestMemo(stateOf(projectId).changes);
	},

	async compare(projectId, path, fromId, toId): Promise<FileDiff> {
		await sleep(WAIT_SHORT);
		const state = stateOf(projectId);
		if (path.endsWith('.psd')) return { kind: 'too_large', sizeBytes: PSD_BYTES };
		if (path.endsWith('.png')) {
			return { kind: 'info', fileKind: 'image', sizeBytes: 184_320, modifiedAt: ago(2 * HOUR) };
		}
		// 新規（未追跡）ファイルを「いま」と比べるときは、全行が追加の差分になる
		const added = state.changes.find((c) => c.path === path);
		if (added?.untracked && toId === 'current') {
			return {
				kind: 'text',
				rows: diffLines(
					[],
					[t('mock.new_file.line1'), t('mock.new_file.line2'), t('mock.new_file.line3')]
				)
			};
		}
		const before = lines(path, tierOf(state, fromId)) ?? [];
		const after = lines(path, tierOf(state, toId)) ?? [];
		if (before.join('\n') === after.join('\n')) return { kind: 'identical' };
		return { kind: 'text', rows: diffLines(before, after) };
	},

	async compareOffice(projectId, path, fromId, toId): Promise<OfficeDiff> {
		await sleep(WAIT_SHORT);
		const kind = officeKindOf(path);
		if (kind === null) return { kind: 'unavailable', reason: 'unsupported_format' };
		if (fromId === toId) {
			return kind === 'docx'
				? { kind: 'docx', rows: [{ kind: 'omitted', count: 4 }], truncated: 0 }
				: { kind: 'xlsx', addedSheets: [], removedSheets: [], cells: [], truncated: 0 };
		}
		if (kind === 'docx') {
			return {
				kind: 'docx',
				rows: [
					{ kind: 'context', oldNo: 1, newNo: 1, text: t('mock.office.title') },
					{ kind: 'omitted', count: 3 },
					{ kind: 'del', oldNo: 5, text: t('mock.office.before') },
					{ kind: 'add', newNo: 5, text: t('mock.office.after') },
					{ kind: 'context', oldNo: 6, newNo: 6, text: t('mock.office.next') },
					{ kind: 'omitted', count: 8 }
				],
				truncated: 0
			};
		}
		// プレビューのモック（mockSheets）と対応する: Sheet2 は削除、Sheet3 は追加、Sheet1 の B3 と C5 が変わる
		return {
			kind: 'xlsx',
			addedSheets: ['Sheet3'],
			removedSheets: ['Sheet2'],
			cells: [
				{ sheet: 'Sheet1', cell: 'B3', row: 2, col: 1, before: '120', after: '150' },
				{
					sheet: 'Sheet1',
					cell: 'C5',
					row: 4,
					col: 2,
					before: null,
					after: t('mock.office.value')
				}
			],
			truncated: 0
		};
	},

	/** ブラウザ（モック）では拡張（ext://）が配信されないため、表示の入口は無い（画面は拡張の失敗の表示になる） */
	async listRenderers(): Promise<RendererInfo[]> {
		await sleep(WAIT_SHORT);
		return [];
	},

	async previewFile(projectId: string, rev: string, path: string): Promise<FilePreview> {
		await sleep(WAIT_SHORT);
		const state = stateOf(projectId);
		const now = rev === 'current' || rev === 'work';
		// ファイルの有無の例: 「古い案.txt」は今の版に無く、「新しい案.txt」は過去の版に無い
		if (now && path === t('mock.file.old_draft')) return { kind: 'not_found' };
		if (!now && path === t('mock.file.draft_new')) return { kind: 'not_found' };
		const office = officeKindOf(path);
		if (office === 'docx') {
			return {
				kind: 'docx',
				paragraphs: [
					t('mock.office.title'),
					'',
					t(now ? 'mock.office.after' : 'mock.office.before'),
					t('mock.office.next')
				],
				truncated: false
			};
		}
		if (office === 'xlsx') return { kind: 'xlsx', sheets: mockSheets(now) };
		const ext = path.split('.').pop()?.toLowerCase() ?? '';
		if (ext === 'pptx') return { kind: 'pptx', slides: mockSlides(), truncated: false };
		if (MOCK_BINARY.includes(ext)) return { kind: 'unavailable', reason: 'binary' };
		if (MOCK_UNSUPPORTED.includes(ext))
			return { kind: 'unavailable', reason: 'unsupported_format' };
		if (MOCK_CORRUPT.includes(ext)) return { kind: 'unavailable', reason: 'corrupt' };
		if (MOCK_TOO_LARGE.includes(ext)) return { kind: 'unavailable', reason: 'too_large' };
		const body = lines(path, tierOf(state, rev)) ?? [t('mock.preview.body')];
		return { kind: 'text', content: body.join('\n'), truncated: false };
	},

	async listProjectTree(projectId: string): Promise<ProjectTree> {
		await sleep(WAIT_SHORT);
		const state = stateOf(projectId);
		const saved = [
			t('mock.file.chapter3'),
			t('mock.file.memo'),
			t('mock.file.refs'),
			t('mock.file.readme'),
			t('mock.file.nested')
		];
		const paths = new Set([...saved, ...state.changes.map((c) => c.path)]);
		const files = [...paths].map((path): ProjectFile => {
			const change = state.changes.find((c) => c.path === path);
			return {
				path,
				sizeBytes: change?.type === 'deleted' ? null : mockSizeOf(path),
				modifiedAt: change?.type === 'deleted' ? null : ago(3 * HOUR),
				change: change?.type ?? null,
				isConflict: change?.isConflict ?? false
			};
		});
		return { files, truncated: false, limit: 10_000 };
	},

	async save(projectId: string, memo: string): Promise<SaveOutcome> {
		await sleep(WAIT_LONG);
		return saveWithCheck(stateOf(projectId), memo);
	},

	async saveWithSizeChoice(projectId, memo, choice): Promise<SaveOutcome> {
		await sleep(WAIT_LONG);
		return saveWithCheck(stateOf(projectId), memo, choice);
	},

	async restoreImpact(projectId, targetId, scope): Promise<ImpactItem[]> {
		await sleep(WAIT_SHORT);
		return impactFor(stateOf(projectId), targetId, scope);
	},

	async restore(projectId, targetId, scope, saveMemo): Promise<RestoreResult> {
		await sleep(WAIT_LONG);
		const state = stateOf(projectId);
		if (scope.kind === 'file' && state.restoreFailuresLeft > 0) {
			state.restoreFailuresLeft -= 1;
			throw new AppError('E12', 'EBUSY: resource busy or locked', { name: scope.path });
		}
		const impact = impactFor(state, targetId, scope);
		const undoToken = nextId('undo');
		undoSnapshots.set(undoToken, snapshotOf(state));
		// 設定「元に戻した後に自動で保存」がオンで、保存のメモが渡されたときだけ、続けて保存する
		const settings = (await mockApi.getSettings(projectId)).settings;
		const wantsSave = settings.autoSaveAfterRestore && !!saveMemo?.trim();
		let save: AfterRestoreSave = 'not_requested';
		if (scope.kind === 'file') {
			state.changes = state.changes.filter((c) => c.path !== scope.path);
			state.project.unsavedCount = state.changes.length;
			if (wantsSave) {
				save = state.changes.length === 0 ? 'nothing_to_save' : 'saved';
				if (save === 'saved') addSavePoint(state, saveMemo ?? '', state.currentTier);
			}
		} else {
			const tier = tierOf(state, targetId);
			if (wantsSave) {
				addSavePoint(state, saveMemo ?? '', tier);
				save = 'saved';
			} else {
				// 保存しないときは、戻した内容が未保存の変更として残る
				state.currentTier = tier;
				state.changes = impact
					.filter((item) => item.type !== 'removed')
					.map((item) => ({
						id: nextId('c'),
						path: item.path,
						type: 'modified' as const,
						isConflict: false,
						untracked: false
					}));
				state.project.unsavedCount = state.changes.length;
			}
		}
		return { undoToken, changedCount: impact.length, save };
	},

	async undoRestore(projectId: string, undoToken: string): Promise<void> {
		await sleep(WAIT_SHORT);
		const snapshot = undoSnapshots.get(undoToken);
		if (!snapshot) return;
		const state = stateOf(projectId);
		state.changes = snapshot.changes;
		state.savePoints = snapshot.savePoints;
		state.tiers = snapshot.tiers;
		state.currentTier = snapshot.currentTier;
		state.project = snapshot.project;
		undoSnapshots.delete(undoToken);
	},

	async discardNewFile(projectId: string, path: string): Promise<DiscardResult> {
		await sleep(WAIT_SHORT);
		const state = stateOf(projectId);
		const target = state.changes.find((c) => c.path === path);
		if (!target?.untracked) {
			throw new AppError(
				'backend',
				'discard refused',
				{},
				{
					code: 'discard_not_untracked',
					params: { file: path },
					whatHappened: t('errcode.discard_not_untracked.what', { file: path }),
					dataIsSafe: t('errcode.discard_not_untracked.safe'),
					nextAction: t('errcode.discard_not_untracked.next')
				}
			);
		}
		// 大きいファイルは復元点に入らないため、削除せずに断る（バックエンドと同じ）
		if (mockSizeOf(path) > LARGE_WARN_BYTES) {
			throw new AppError(
				'backend',
				'discard refused',
				{},
				{
					code: 'discard_file_too_large',
					params: { file: path },
					whatHappened: t('errcode.discard_file_too_large.what', { file: path }),
					dataIsSafe: t('errcode.discard_file_too_large.safe'),
					nextAction: t('errcode.discard_file_too_large.next')
				}
			);
		}
		const undoToken = nextId('undo');
		undoSnapshots.set(undoToken, snapshotOf(state));
		state.changes = state.changes.filter((c) => c.path !== path);
		state.project.unsavedCount = state.changes.length;
		return { undoToken };
	},

	async listIgnoreRules(projectId: string): Promise<IgnoreRule[]> {
		await sleep(WAIT_SHORT);
		// 件数は実際のファイルを調べないため 0 にする（ブラウザ単体の確認用）
		return ignorePatternsOf(projectId).map((pattern) => ({ pattern, trackedCount: 0 }));
	},

	async listTrackedIgnoredFiles(projectId: string): Promise<string[]> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		return [];
	},

	async addIgnorePattern(projectId: string, kind: IgnoreKind, value: string): Promise<string> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		const pattern = mockIgnorePattern(kind, value);
		const patterns = ignorePatternsOf(projectId);
		if (!patterns.includes(pattern)) ignorePatterns.set(projectId, [...patterns, pattern]);
		return pattern;
	},

	async removeIgnorePattern(projectId: string, pattern: string): Promise<void> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		const patterns = ignorePatternsOf(projectId);
		const index = patterns.indexOf(pattern);
		if (index < 0) {
			throw new AppError(
				'backend',
				'pattern not found',
				{},
				{
					code: 'invalid_input',
					params: {},
					whatHappened: t('errcode.invalid_input.what'),
					dataIsSafe: t('errcode.invalid_input.safe'),
					nextAction: t('errcode.invalid_input.next')
				}
			);
		}
		ignorePatterns.set(
			projectId,
			patterns.filter((_, i) => i !== index)
		);
	},

	async untrackFiles(projectId: string): Promise<void> {
		await sleep(WAIT_LONG);
		stateOf(projectId);
	},

	async listSkipWorktree(projectId: string): Promise<SkipWorktreeEntry[]> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		// ブラウザ単体では git の印を持たないため、記録の内容を無視中として返す
		return skipRecordsOf(projectId).map((r) => ({
			path: r.path,
			setAt: r.setAt,
			ignored: true,
			dismissed: r.dismissed
		}));
	},

	async listSkipWorktreeDrift(projectId: string): Promise<string[]> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		return [];
	},

	async dismissSkipWorktreeDrift(projectId: string, paths: string[]): Promise<void> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		skipRecords.set(
			projectId,
			skipRecordsOf(projectId).map((r) => (paths.includes(r.path) ? { ...r, dismissed: true } : r))
		);
	},

	async setSkipWorktree(projectId: string, paths: string[]): Promise<void> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		setMockSkipRecords(projectId, paths);
	},

	async clearSkipWorktree(projectId: string, paths: string[]): Promise<void> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		skipRecords.set(
			projectId,
			skipRecordsOf(projectId).filter((r) => !paths.includes(r.path))
		);
	},

	async reapplySkipWorktree(projectId: string, paths: string[]): Promise<void> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		setMockSkipRecords(projectId, paths);
	},

	async listSkipStash(projectId: string): Promise<SkipStashList> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		return { items: [...skipStashesOf(projectId)], skippedInvalid: 0 };
	},

	async extractSkipStash(projectId: string, stashId: string): Promise<SkipStashExtracted> {
		await sleep(WAIT_SHORT);
		stateOf(projectId);
		const list = skipStashesOf(projectId);
		const entry = list.find((item) => item.id === stashId);
		if (!entry) {
			throw new AppError(
				'backend',
				'saved copy not found',
				{},
				{
					code: 'invalid_input',
					params: {},
					whatHappened: t('errcode.invalid_input.what'),
					dataIsSafe: t('errcode.invalid_input.safe'),
					nextAction: t('errcode.invalid_input.next')
				}
			);
		}
		// 元のファイルと同じフォルダに、別名（ラベルつき）で置く。ブラウザ単体では実ファイルは作らない
		const slash = entry.path.lastIndexOf('/');
		const dir = entry.path.slice(0, slash + 1);
		const name = entry.path.slice(slash + 1);
		const dot = name.lastIndexOf('.');
		const stem = dot > 0 ? name.slice(0, dot) : name;
		const ext = dot > 0 ? name.slice(dot) : '';
		const date = new Date();
		const mmdd = `${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
		skipStashes.set(
			projectId,
			list.filter((item) => item.id !== stashId)
		);
		return { copyPath: `${dir}${stem} (${t('skip_stash.copy_label')} ${mmdd})${ext}` };
	},

	async fetch(projectId: string, options?: FetchOptions): Promise<FetchResult> {
		await sleep(WAIT_LONG + 300);
		const state = stateOf(projectId);
		const settings = readStorage<AppSettings>('hikae.settings', DEFAULT_SETTINGS);
		const confirmed = options?.saveConfirmed ?? false;
		if (state.changes.length > 0 && !settings.saveBeforePull && !confirmed) {
			// 「確認する」設定: 取り込む内容があるときだけ、何も実行せずに確認を求める
			if (state.project.fetchPendingCount > 0) {
				return {
					mergedCount: 0,
					conflictCount: 0,
					sizeCheck: null,
					unsavedCount: state.changes.length
				};
			}
			return { mergedCount: 0, conflictCount: 0, sizeCheck: null, unsavedCount: null };
		}
		if (state.changes.length > 0) {
			// 取り込み前の自動保存に大きいファイルがあるときは、何も実行せずに見送る
			const check = inspectSizes(state.changes);
			if (check.blocked.length > 0 || check.warned.length > 0) {
				return { mergedCount: 0, conflictCount: 0, sizeCheck: check, unsavedCount: null };
			}
			addSavePoint(state, t('mock.memo.before_fetch'), 3);
		}
		if (state.conflictsOnNextFetch && state.conflicts.length > 0) {
			state.conflictsOnNextFetch = false;
			state.project.hasConflict = true;
			return {
				mergedCount: 0,
				conflictCount: state.conflicts.length,
				sizeCheck: null,
				unsavedCount: null
			};
		}
		const merged = state.project.fetchPendingCount;
		state.project.fetchPendingCount = 0;
		return { mergedCount: merged, conflictCount: 0, sizeCheck: null, unsavedCount: null };
	},

	async push(projectId: string): Promise<PushResult> {
		await sleep(WAIT_LONG + 300);
		const state = stateOf(projectId);
		if (state.pushFailuresLeft > 0) {
			state.pushFailuresLeft -= 1;
			throw new AppError('E04', 'remote: Internal Server Error (HTTP 500)');
		}
		state.project.uploadPendingCount = 0;
		state.project.lastUploadedAt = new Date();
		state.savePoints = state.savePoints.map((s) => ({ ...s, cloudSynced: true }));
		return { sizeCheck: null };
	},

	async listConflicts(projectId: string): Promise<ConflictFile[]> {
		await sleep(200);
		return stateOf(projectId).conflicts.map((c) => ({ ...c }));
	},

	async conflictText(projectId: string, path: string): Promise<string | null> {
		await sleep(WAIT_SHORT);
		const conflict = stateOf(projectId).conflicts.find((c) => c.path === path);
		// 両方変更・両方追加だけ全文を返す（モックでは行ごとの解消の画面を確かめられるように短い全文を返す）
		if (!conflict || conflict.kind !== 'both') return null;
		return [
			'<<<<<<< HEAD',
			'line from this PC',
			'=======',
			'line from cloud',
			'>>>>>>> cloud',
			'common line',
			''
		].join('\n');
	},

	async resolveConflicts(projectId: string, resolutions: ConflictResolution[]): Promise<void> {
		await sleep(WAIT_LONG);
		const state = stateOf(projectId);
		if (resolutions.length < state.conflicts.length) {
			throw new AppError('E05', 'unresolved paths remain');
		}
		state.conflicts = [];
		state.project.hasConflict = false;
		state.project.fetchPendingCount = 0;
		addSavePoint(state, t('mock.memo.resolved'), 3);
	},

	async abortMerge(projectId: string): Promise<void> {
		await sleep(WAIT_SHORT);
		const state = stateOf(projectId);
		state.project.hasConflict = false;
		state.conflictsOnNextFetch = true;
	},

	async addFiles(
		projectId: string,
		files: DroppedFile[],
		options: AddFilesOptions = {}
	): Promise<AddFilesOutcome> {
		await sleep(WAIT_SHORT);
		const state = stateOf(projectId);
		const { accepted, rejected } = classifyDropped(files);
		const folder = options.destSubdir ? `${options.destSubdir}/` : '';
		const policy = options.policy ?? 'ask';
		const decisions = new Map((options.decisions ?? []).map((d) => [d.path, d.action]));
		const taken = new Set([
			...state.changes.map((c) => c.path),
			...[t('mock.file.chapter3'), t('mock.file.memo')]
		]);
		const noResult = {
			added: [],
			rejected: [],
			skipped: [],
			needsDecision: [],
			undoToken: null
		};
		// 同名があれば、方針が「確認する」のとき何も書かずに一覧を返す
		const conflicts = accepted
			.filter((file) => taken.has(folder + file.name))
			.map((file) => ({
				path: folder + file.name,
				canReplace: (file.size ?? 0) < LARGE_WARN_BYTES
			}));
		const unanswered = conflicts.filter((c) => !decisions.has(c.path));
		if (policy === 'ask' && unanswered.length > 0) {
			return { ...noResult, needsDecision: unanswered };
		}
		let undoToken: string | null = null;
		const added: AddFilesOutcome['added'] = [];
		for (const file of accepted) {
			const wanted = folder + file.name;
			const large = (file.size ?? 0) >= LARGE_WARN_BYTES;
			let path = wanted;
			let replaced = false;
			let replaceRefused = false;
			if (taken.has(wanted)) {
				const choice = decisions.get(wanted) ?? (policy === 'replace' ? 'replace' : 'keep_both');
				if (choice === 'skip') continue;
				const canReplace = conflicts.find((c) => c.path === wanted)?.canReplace ?? false;
				if (choice === 'replace' && canReplace) {
					// 置き換え前の状態を取り消し用に控える
					if (undoToken === null) {
						undoToken = nextId('undo');
						undoSnapshots.set(undoToken, snapshotOf(state));
					}
					replaced = true;
				} else {
					replaceRefused = choice === 'replace';
					let counter = 1;
					while (taken.has(path)) {
						counter += 1;
						const dot = wanted.lastIndexOf('.');
						path =
							dot > folder.length
								? `${wanted.slice(0, dot)} (${counter})${wanted.slice(dot)}`
								: `${wanted} (${counter})`;
					}
				}
			}
			taken.add(path);
			if (!replaced && !state.changes.some((c) => c.path === path)) {
				state.changes.push({
					id: nextId('c'),
					path,
					type: 'added',
					isConflict: false,
					untracked: true
				});
			}
			added.push({ path, renamed: path !== wanted, large, replaced, replaceRefused });
		}
		state.project.unsavedCount = state.changes.length;
		return {
			...noResult,
			added,
			undoToken,
			rejected: rejected.map((file) => ({
				name: file.name,
				reason: 'too_large' as const,
				size: file.size
			}))
		};
	},

	async checkProjectFolder(path: string): Promise<FolderCheck> {
		await sleep(40);
		// 画面確認用の簡易判定（実際の判定はバックエンド）。ドライブのルートとホームフォルダだけ
		const normalized = path.replaceAll('\\', '/').replace(/\/+$/, '');
		// 同期フォルダの目印になる名前（OneDrive・Dropbox・iCloud Drive・Google Drive）
		const synced = /(^|\/)(OneDrive|Dropbox|iCloud ?Drive|Google Drive)(\/| - |-|$)/i.test(
			normalized
		);
		const warnings: FolderWarning[] = synced ? ['cloud_sync'] : [];
		if (normalized === '' || /^[A-Za-z]:$/.test(normalized))
			return { broad: 'drive_root', warnings };
		if (/^(C:)?\/(Users|home)\/[^/]+$/i.test(normalized)) return { broad: 'home', warnings };
		return { broad: null, warnings };
	},

	async getAppHealth(): Promise<AppHealth> {
		await sleep(20);
		// 画面確認用: localStorage の hikae.mock.parts を "missing" にすると、部品が無い状態になる
		let missing = false;
		try {
			missing = localStorage.getItem('hikae.mock.parts') === 'missing';
		} catch {
			// 読めなければ、部品はある状態にする
		}
		return {
			partsAvailable: !missing,
			partsSource: 'bundled',
			partsProblem: missing ? 'not_found' : null
		};
	},

	async openFile(): Promise<void> {
		await sleep(80);
	},

	async openFileAt(): Promise<void> {
		await sleep(80);
	},

	async recoverInterrupted(projectId: string): Promise<InterruptedRecovery> {
		await sleep(WAIT_LONG);
		stateOf(projectId).project.interruptedOperation = null;
		return 'recovered';
	},

	async dismissInterrupted(projectId: string): Promise<void> {
		await sleep(WAIT_LONG);
		stateOf(projectId).project.interruptedOperation = null;
	},

	async takeClosedInterruptedNotices(): Promise<string[]> {
		return [];
	},

	async getSettings(projectId: string | null): Promise<SettingsView> {
		const global = readStorage<AppSettings>('hikae.settings', DEFAULT_SETTINGS);
		if (projectId === null) return { settings: global, overridden: [] };
		const override = readOverrides()[projectId] ?? {};
		return {
			settings: { ...global, ...override },
			overridden: Object.keys(override) as SettingKey[]
		};
	},

	async updateSettings(
		projectId: string | null,
		patch: Partial<AppSettings>
	): Promise<SettingsView> {
		if (projectId === null) {
			const next = { ...readStorage<AppSettings>('hikae.settings', DEFAULT_SETTINGS), ...patch };
			writeStorage('hikae.settings', next);
		} else {
			const all = readOverrides();
			writeStorage('hikae.settings.projects', {
				...all,
				[projectId]: { ...(all[projectId] ?? {}), ...patch }
			});
		}
		return mockApi.getSettings(projectId);
	},

	async aiCatalog(): Promise<AiCatalog> {
		await sleep(80);
		const saved = readAiStore();
		return {
			system: { totalMemoryBytes: MOCK_AI_MEMORY, freeDiskBytes: MOCK_AI_DISK },
			models: MOCK_AI_MODELS.map((model) => {
				const record = saved[model.id] ?? { status: 'missing', partialBytes: 0 };
				return {
					...model,
					status: record.status,
					partialBytes: record.partialBytes,
					available:
						model.minMemoryBytes <= MOCK_AI_MEMORY && model.sizeBytes * 1.1 <= MOCK_AI_DISK,
					unavailableReason: null,
					downloading: aiRunning.has(model.id)
				};
			})
		};
	},

	async aiDownloadModel(id: string): Promise<void> {
		const model = MOCK_AI_MODELS.find((m) => m.id === id);
		if (!model) throw new Error(`unknown model: ${id}`);
		if (aiRunning.has(id)) throw new Error(`already downloading: ${id}`);
		// 実際のファイルは作らず、受信量を時間で進めるだけ
		const progress = { received: readAiStore()[id]?.partialBytes ?? 0, total: model.sizeBytes };
		const timer = setInterval(() => {
			progress.received = Math.min(progress.total, progress.received + progress.total / 20);
			emitAi({ kind: 'progress', id, received: progress.received, total: progress.total });
			if (progress.received >= progress.total) {
				clearInterval(timer);
				aiRunning.delete(id);
				writeAiRecord(id, { status: 'ready', partialBytes: progress.total });
				emitAi({ kind: 'finished', id, outcome: 'ready' });
			}
		}, 200);
		aiRunning.set(id, { timer, progress });
	},

	async aiCancelDownload(id: string): Promise<void> {
		const run = aiRunning.get(id);
		if (!run) return;
		clearInterval(run.timer);
		aiRunning.delete(id);
		writeAiRecord(id, { status: 'partial', partialBytes: run.progress.received });
		emitAi({ kind: 'finished', id, outcome: 'canceled' });
	},

	async aiRemoveModel(id: string): Promise<void> {
		if (aiRunning.has(id)) throw new Error(`downloading: ${id}`);
		writeAiRecord(id, { status: 'missing', partialBytes: 0 });
		const settings = readStorage<AppSettings>('hikae.settings', DEFAULT_SETTINGS);
		if (settings.aiModel === id) writeStorage('hikae.settings', { ...settings, aiModel: '' });
	},

	async aiSuggestMemo(projectId: string): Promise<AiSuggestion> {
		// 新しい要求は進行中の要求を取り消す（実装と同じ）
		const generation = ++mockSuggestGeneration;
		await sleep(120);
		if (generation !== mockSuggestGeneration)
			return { title: null, body: null, notices: [], failure: 'cancelled' };
		const overrides = readOverrides()[projectId] ?? {};
		const settings = {
			...readStorage<AppSettings>('hikae.settings', DEFAULT_SETTINGS),
			...overrides
		};
		if (settings.memoSuggestion !== 'ai' || !mockModelReady(settings)) {
			return { title: null, body: null, notices: [], failure: 'not-ready' };
		}
		const state = states.get(projectId);
		if (!state || state.changes.length === 0)
			return { title: null, body: null, notices: [], failure: 'not-ready' };
		return {
			title: `AI 提案（モック）: ${suggestMemo(state.changes)}`,
			body: '（モック）詳細の 1 行目\n（モック）詳細の 2 行目',
			failure: null,
			notices: []
		};
	},

	async checkMemoSensitive(): Promise<SensitiveKind[]> {
		return [];
	},

	async aiCancelSuggest(): Promise<void> {
		mockSuggestGeneration++;
	},

	async aiListOllamaModels(url: string): Promise<string[]> {
		await sleep(150);
		if (!/^https?:\/\//.test(url)) throw new Error('invalid endpoint');
		return ['gemma3:4b', 'qwen3:4b'];
	},

	async aiCheckEndpoint(url: string): Promise<AiEndpointCheck> {
		await sleep(120);
		return {
			ok: /^https?:\/\//.test(url),
			externalHost: !/^https?:\/\/(localhost|127\.0\.0\.1)(:|\/|$)/.test(url)
		};
	},

	async aiTestGeneration(): Promise<AiTestResult> {
		await sleep(300);
		const settings = readStorage<AppSettings>('hikae.settings', DEFAULT_SETTINGS);
		if (settings.aiRunner === 'builtin' && settings.aiModelSource === 'custom-gguf') {
			if (settings.aiCustomGgufPath === '') return { ok: false, reason: 'not-configured' };
			return mockCustomGgufCheck(settings.aiCustomGgufPath).ok
				? { ok: true, reason: null }
				: { ok: false, reason: 'custom-invalid' };
		}
		if (settings.aiModel === '') return { ok: false, reason: 'not-configured' };
		if (settings.aiRunner === 'builtin' && readAiStore()[settings.aiModel]?.status !== 'ready') {
			return { ok: false, reason: 'not-installed' };
		}
		return { ok: true, reason: null };
	},

	async aiCheckCustomGguf(path: string): Promise<AiCustomGgufCheck> {
		await sleep(120);
		return mockCustomGgufCheck(path);
	},

	onAiDownload(listener: (notice: AiDownloadNotice) => void): () => void {
		aiListeners.add(listener);
		return () => {
			aiListeners.delete(listener);
		};
	}
};

// ---- AI（ローカル LLM）のモック。ダウンロードは時間で進むだけで、ファイルは作らない ----

const GIB = 1024 * 1024 * 1024;
const MOCK_AI_MEMORY = 16 * GIB;
const MOCK_AI_DISK = 120 * GIB;

/** モックのモデルが使えるか（内蔵はカタログの導入済み、またはカスタムのパス。Ollama はモデル名）。 */
function mockModelReady(settings: AppSettings): boolean {
	if (settings.aiRunner === 'ollama') return settings.aiModel !== '';
	if (settings.aiModelSource === 'custom-gguf') return settings.aiCustomGgufPath !== '';
	return settings.aiModel !== '' && readAiStore()[settings.aiModel]?.status === 'ready';
}

/** モックのカスタム GGUF の検査（実装の順序に近い形。ファイルは読まない） */
function mockCustomGgufCheck(path: string): AiCustomGgufCheck {
	if (path === '') return { ok: false, reason: 'empty' };
	if (path.split('').some((c) => c.charCodeAt(0) < 32 || c.charCodeAt(0) === 127)) {
		return { ok: false, reason: 'control-char' };
	}
	if (path.includes('missing')) return { ok: false, reason: 'not-found' };
	if (!path.toLowerCase().endsWith('.gguf')) return { ok: false, reason: 'wrong-extension' };
	return { ok: true, reason: null };
}

/** カタログの内容（バックエンドの catalog.json と同じ値・同じ順） */
const MOCK_AI_MODELS: Omit<
	AiModel,
	'status' | 'partialBytes' | 'available' | 'unavailableReason' | 'downloading'
>[] = [
	{
		id: 'qwen3.5-2b-q4_k_m',
		displayName: 'Qwen3.5-2B（Q4_K_M）',
		stage: 1,
		sizeBytes: 1280835840,
		licenseName: 'Apache License 2.0',
		licenseUrl: 'https://www.apache.org/licenses/LICENSE-2.0',
		minMemoryBytes: 7516192768,
		recommended: false
	},
	{
		id: 'gemma-4-e2b-it-q4_k_m',
		displayName: 'Gemma 4 E2B it（Q4_K_M）',
		stage: 2,
		sizeBytes: 3106738272,
		licenseName: 'Apache License 2.0',
		licenseUrl: 'https://www.apache.org/licenses/LICENSE-2.0',
		minMemoryBytes: 7516192768,
		recommended: true
	},
	{
		id: 'qwen3-4b-instruct-2507-q4_k_m',
		displayName: 'Qwen3-4B-Instruct-2507（Q4_K_M）',
		stage: 2,
		sizeBytes: 2497281120,
		licenseName: 'Apache License 2.0',
		licenseUrl: 'https://www.apache.org/licenses/LICENSE-2.0',
		minMemoryBytes: 7516192768,
		recommended: false
	}
];

const aiListeners = new Set<(notice: AiDownloadNotice) => void>();
/** モックの提案の要求番号（新しい要求・取り消しで増やし、古い要求の結果を捨てる） */
let mockSuggestGeneration = 0;
const aiRunning = new Map<
	string,
	{ timer: ReturnType<typeof setInterval>; progress: { received: number; total: number } }
>();

function readAiStore(): Record<string, { status: AiModelStatus; partialBytes: number }> {
	return readStorage<Record<string, { status: AiModelStatus; partialBytes: number }>>(
		'hikae.mock.ai',
		{}
	);
}

function writeAiRecord(id: string, record: { status: AiModelStatus; partialBytes: number }): void {
	writeStorage('hikae.mock.ai', { ...readAiStore(), [id]: record });
}

function emitAi(notice: AiDownloadNotice): void {
	for (const listener of aiListeners) listener(notice);
}
