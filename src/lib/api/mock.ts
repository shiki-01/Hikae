import { t } from '#lib/i18n/index.js';
import { AppError } from './errors';
import { suggestMemo } from '#lib/features/changes/memo.js';
import { diffLines } from './diff-lines';
import { classifyDropped, LARGE_WARN_BYTES, MAX_FILE_BYTES } from '#lib/features/changes/files.js';
import type {
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
	autoSaveAfterRestore: true,
	largeFileWarnMb: 50,
	showSnapshotsInTimeline: 'collapsed'
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

	async recoverInterrupted(projectId: string): Promise<void> {
		await sleep(WAIT_LONG);
		stateOf(projectId).project.interruptedOperation = null;
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
	}
};
