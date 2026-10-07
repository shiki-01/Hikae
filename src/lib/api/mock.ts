import { t } from '#lib/i18n/index.js';
import { formatDateTime } from '#lib/i18n/format.js';
import { AppError } from './errors';
import { diffLines } from './diff-lines';
import type {
	AddProjectInput,
	AppSettings,
	Change,
	ConflictFile,
	ConflictResolution,
	DeviceFlow,
	DroppedFile,
	FetchResult,
	FileDiff,
	ImpactItem,
	Owner,
	PointFile,
	Project,
	ProjectApi,
	RemoteProject,
	RestoreResult,
	RestoreScope,
	SavePoint,
	Session
} from './types';

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const LARGE_FILE_BYTES = 72 * 1024 * 1024;
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
			lastUploadedAt: ago(DAY),
			unsavedCount: 4,
			uploadPendingCount: 1,
			fetchPendingCount: 2,
			hasConflict: false,
			folderMissing: false
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
			{ id: 'c2', path: t('mock.file.fig4'), type: 'added', isConflict: false },
			{ id: 'c3', path: t('mock.file.old_draft'), type: 'deleted', isConflict: false },
			{ id: 'c4', path: t('mock.file.logo'), type: 'added', isConflict: false }
		],
		conflicts: [
			{
				path: chapter,
				kind: 'both',
				thisPcSavedAt: ago(14 * HOUR),
				cloudSavedAt: ago(17 * HOUR),
				cloudPcName: t('mock.pc.laptop')
			},
			{
				path: t('mock.file.memo'),
				kind: 'deleted_in_cloud',
				thisPcSavedAt: ago(15 * HOUR),
				cloudSavedAt: ago(16 * HOUR),
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
			lastUploadedAt: ago(5 * DAY),
			unsavedCount: 0,
			uploadPendingCount: 1,
			fetchPendingCount: 0,
			hasConflict: false,
			folderMissing: false
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
			lastUploadedAt: ago(9 * DAY),
			unsavedCount: 0,
			uploadPendingCount: 0,
			fetchPendingCount: 0,
			hasConflict: false,
			folderMissing: true
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

const states = new Map<string, ProjectState>();
for (const state of [createThesis(), createMaterials(), createAlbum()]) {
	states.set(state.project.id, state);
}
const undoSnapshots = new Map<string, Snapshot>();
let sequence = 100;

const DEFAULT_SETTINGS: AppSettings = {
	autoSaveBeforeFetch: true,
	autoFetchOnLaunch: true,
	autoUploadOnSave: false,
	autoSaveAfterRestore: true
};

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
	async getSession(): Promise<Session> {
		await sleep(80);
		return readStorage<Session>('hikae.session', { loggedIn: false, onboarded: false });
	},

	async startLogin(): Promise<DeviceFlow> {
		await sleep(WAIT_SHORT);
		return { userCode: 'WDJB-MJHT' };
	},

	async waitLogin(): Promise<void> {
		await sleep(3000);
		writeStorage('hikae.session', { loggedIn: true, onboarded: false });
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
			{ id: 'personal', name: 'shiki-01', kind: 'personal', canCreate: true },
			{ id: 'org-hikae', name: 'hikae-app', kind: 'org', canCreate: true },
			{ id: 'org-campus', name: 'campus-lab', kind: 'org', canCreate: false }
		];
	},

	async listRemoteProjects(query: string): Promise<RemoteProject[]> {
		await sleep(WAIT_SHORT);
		const all: RemoteProject[] = [
			{ id: 'r1', name: t('mock.remote.research'), ownerId: 'personal' },
			{ id: 'r2', name: t('mock.remote.recipes'), ownerId: 'personal' },
			{ id: 'r3', name: t('mock.remote.novel'), ownerId: 'org-hikae' },
			{ id: 'r4', name: t('mock.remote.minutes'), ownerId: 'org-campus' }
		];
		const needle = query.trim().toLowerCase();
		return needle ? all.filter((r) => r.name.toLowerCase().includes(needle)) : all;
	},

	async pickFolder(): Promise<string> {
		await sleep(150);
		return 'C:\\Users\\student\\Documents\\NewFolder';
	},

	async addProject(input: AddProjectInput): Promise<Project> {
		await sleep(WAIT_LONG);
		const id = nextId('proj');
		const owners = await mockApi.listOwners();
		const owner = owners.find((o) => o.id === input.ownerId) ?? owners[0];
		const project: Project = {
			id,
			name: input.name,
			path: input.mode === 'existing' ? input.folder : `${input.folder}\\${input.name}`,
			ownerName: owner.name,
			ownerKind: owner.kind,
			lastSavedAt: new Date(),
			lastUploadedAt: input.mode === 'github' ? new Date() : null,
			unsavedCount: 0,
			uploadPendingCount: input.mode === 'github' ? 0 : 1,
			fetchPendingCount: 0,
			hasConflict: false,
			folderMissing: false
		};
		states.set(id, {
			project,
			changes: [],
			savePoints: [
				{
					id: nextId('sp'),
					createdAt: new Date(),
					message: t('mock.memo.s5'),
					kind: 'save',
					cloudSynced: input.mode === 'github'
				}
			],
			tiers: new Map(),
			currentTier: 3,
			conflicts: [],
			conflictsOnNextFetch: false,
			pushFailuresLeft: 0,
			restoreFailuresLeft: 0
		});
		return { ...project };
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

	async listSavePoints(projectId: string): Promise<SavePoint[]> {
		await sleep(WAIT_SHORT);
		return stateOf(projectId).savePoints.map((s) => ({ ...s }));
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

	async compare(projectId, path, fromId, toId): Promise<FileDiff> {
		await sleep(WAIT_SHORT);
		const state = stateOf(projectId);
		if (path.endsWith('.psd')) return { kind: 'too_large', sizeBytes: LARGE_FILE_BYTES };
		if (path.endsWith('.png')) {
			return { kind: 'info', fileKind: 'image', sizeBytes: 184_320, modifiedAt: ago(2 * HOUR) };
		}
		const before = lines(path, tierOf(state, fromId)) ?? [];
		const after = lines(path, tierOf(state, toId)) ?? [];
		if (before.join('\n') === after.join('\n')) return { kind: 'identical' };
		return { kind: 'text', rows: diffLines(before, after) };
	},

	async save(projectId: string, memo: string): Promise<void> {
		await sleep(WAIT_LONG);
		addSavePoint(stateOf(projectId), memo, 3);
	},

	async restoreImpact(projectId, targetId, scope): Promise<ImpactItem[]> {
		await sleep(WAIT_SHORT);
		return impactFor(stateOf(projectId), targetId, scope);
	},

	async restore(projectId, targetId, scope): Promise<RestoreResult> {
		await sleep(WAIT_LONG);
		const state = stateOf(projectId);
		if (scope.kind === 'file' && state.restoreFailuresLeft > 0) {
			state.restoreFailuresLeft -= 1;
			throw new AppError('E12', 'EBUSY: resource busy or locked', { name: scope.path });
		}
		const impact = impactFor(state, targetId, scope);
		const undoToken = nextId('undo');
		undoSnapshots.set(undoToken, snapshotOf(state));
		if (scope.kind === 'file') {
			state.changes = state.changes.filter((c) => c.path !== scope.path);
			state.project.unsavedCount = state.changes.length;
		} else {
			const target = state.savePoints.find((s) => s.id === targetId);
			addSavePoint(
				state,
				t('mock.memo.restored', {
					time: formatDateTime(target?.createdAt ?? new Date()),
					count: impact.length
				}),
				tierOf(state, targetId)
			);
		}
		return { undoToken, changedCount: impact.length };
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

	async fetch(projectId: string): Promise<FetchResult> {
		await sleep(WAIT_LONG + 300);
		const state = stateOf(projectId);
		const settings = readStorage<AppSettings>('hikae.settings', DEFAULT_SETTINGS);
		if (state.changes.length > 0 && settings.autoSaveBeforeFetch) {
			addSavePoint(state, t('mock.memo.before_fetch'), 3);
		}
		if (state.conflictsOnNextFetch && state.conflicts.length > 0) {
			state.conflictsOnNextFetch = false;
			state.project.hasConflict = true;
			return { mergedCount: 0, conflictCount: state.conflicts.length };
		}
		const merged = state.project.fetchPendingCount;
		state.project.fetchPendingCount = 0;
		return { mergedCount: merged, conflictCount: 0 };
	},

	async push(projectId: string): Promise<void> {
		await sleep(WAIT_LONG + 300);
		const state = stateOf(projectId);
		if (state.pushFailuresLeft > 0) {
			state.pushFailuresLeft -= 1;
			throw new AppError('E04', 'remote: Internal Server Error (HTTP 500)');
		}
		state.project.uploadPendingCount = 0;
		state.project.lastUploadedAt = new Date();
		state.savePoints = state.savePoints.map((s) => ({ ...s, cloudSynced: true }));
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

	async addFiles(projectId: string, files: DroppedFile[]): Promise<number> {
		await sleep(WAIT_SHORT);
		const state = stateOf(projectId);
		for (const file of files) {
			state.changes.push({ id: nextId('c'), path: file.name, type: 'added', isConflict: false });
		}
		state.project.unsavedCount = state.changes.length;
		return files.length;
	},

	async openFile(): Promise<void> {
		await sleep(80);
	},

	async getSettings(): Promise<AppSettings> {
		return readStorage<AppSettings>('hikae.settings', DEFAULT_SETTINGS);
	},

	async updateSettings(patch: Partial<AppSettings>): Promise<AppSettings> {
		const next = { ...readStorage<AppSettings>('hikae.settings', DEFAULT_SETTINGS), ...patch };
		writeStorage('hikae.settings', next);
		return next;
	}
};
