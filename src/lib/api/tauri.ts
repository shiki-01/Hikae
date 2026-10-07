import { open } from '@tauri-apps/plugin-dialog';
import { openPath } from '@tauri-apps/plugin-opener';
import { commands } from '#lib/bindings.js';
import { mockApi } from './mock';
import { resolveInProject } from './paths';
import {
	mapChange,
	mapConflict,
	mapDiff,
	mapError,
	mapImpact,
	mapProject,
	mapPull,
	mapResolutions,
	mapSavePoint,
	parseDate
} from './mappers';
import type {
	AddProjectInput,
	Change,
	ConflictFile,
	FileDiff,
	ImpactItem,
	OpenTarget,
	PointFile,
	Project,
	ProjectApi,
	RestoreResult,
	RestoreScope,
	SavePoint
} from './types';

const HISTORY_LIMIT = 200;

type Result<T> =
	{ status: 'ok'; data: T } | { status: 'error'; error: Parameters<typeof mapError>[0] };

/** Result 形式の戻り値を展開し、エラーは AppError として throw する */
async function unwrap<T>(promise: Promise<Result<T>>): Promise<T> {
	const result = await promise;
	if (result.status === 'error') throw mapError(result.error);
	return result.data;
}

/** バックエンド未対応の機能。無言で成功させず、明示的に失敗させる */
function unsupported(name: string): never {
	throw new Error(`unsupported: ${name} is not implemented in the backend yet`);
}

async function loadProject(id: string): Promise<Project> {
	const infos = await unwrap(commands.listProjects());
	const info = infos.find((item) => item.id === id);
	if (!info) throw new Error(`project not found: ${id}`);
	return buildProject(info);
}

async function buildProject(info: Parameters<typeof mapProject>[0]): Promise<Project> {
	const [status, history] = await Promise.all([
		unwrap(commands.projectStatus(info.id)),
		unwrap(commands.listHistory(info.id, HISTORY_LIMIT))
	]);
	const lastSave = history.find((item) => !item.is_snapshot) ?? history[0];
	return mapProject(info, status, lastSave ? parseDate(lastSave.timestamp) : null);
}

function requireAllScope(scope: RestoreScope): void {
	if (scope.kind !== 'all') unsupported('restore of a single file');
}

export const tauriApi: ProjectApi = {
	// ログイン・初回設定・設定値はバックエンド未実装のため、モックに委ねる
	getSession: () => mockApi.getSession(),
	startLogin: () => mockApi.startLogin(),
	waitLogin: () => mockApi.waitLogin(),
	completeOnboarding: () => mockApi.completeOnboarding(),
	listOwners: () => mockApi.listOwners(),
	listRemoteProjects: (query) => mockApi.listRemoteProjects(query),
	getSettings: () => mockApi.getSettings(),
	updateSettings: (patch) => mockApi.updateSettings(patch),

	async listProjects(): Promise<Project[]> {
		const infos = await unwrap(commands.listProjects());
		return Promise.all(infos.map(buildProject));
	},

	getProject: loadProject,

	async pickFolder(): Promise<string | null> {
		const selected = await open({ directory: true, multiple: false });
		return typeof selected === 'string' ? selected : null;
	},

	async addProject(input: AddProjectInput): Promise<Project> {
		if (input.mode === 'github') unsupported('adding a project from GitHub');
		const id = crypto.randomUUID();
		await unwrap(commands.addProject(id, input.name, input.folder, input.ownerId, null));
		return loadProject(id);
	},

	async removeProject(id: string): Promise<void> {
		await unwrap(commands.removeProject(id));
	},

	async relocateProject(): Promise<Project> {
		return unsupported('relocate project');
	},

	async listChanges(projectId: string): Promise<Change[]> {
		const [files, conflicts] = await Promise.all([
			unwrap(commands.listChanges(projectId)),
			unwrap(commands.listConflicts(projectId))
		]);
		const conflictPaths = new Set(conflicts.map((item) => item.path));
		return files.map((file) => mapChange(file, conflictPaths));
	},

	async listSavePoints(projectId: string): Promise<SavePoint[]> {
		const items = await unwrap(commands.listHistory(projectId, HISTORY_LIMIT));
		return items.map(mapSavePoint);
	},

	async listPointFiles(): Promise<PointFile[]> {
		// その時点で変更されたファイルの一覧を返すコマンドが未提供
		return [];
	},

	async compare(projectId: string, path: string, fromId: string, toId: string): Promise<FileDiff> {
		const lines = await unwrap(commands.diff(projectId, fromId, toId, path));
		return mapDiff(lines);
	},

	async save(projectId: string, memo: string): Promise<void> {
		await unwrap(commands.save(projectId, memo));
	},

	async restoreImpact(
		projectId: string,
		targetId: string,
		scope: RestoreScope
	): Promise<ImpactItem[]> {
		requireAllScope(scope);
		return mapImpact(await unwrap(commands.restorePreview(projectId, targetId)));
	},

	async restore(projectId: string, targetId: string, scope: RestoreScope): Promise<RestoreResult> {
		requireAllScope(scope);
		const preview = await unwrap(commands.restorePreview(projectId, targetId));
		await unwrap(commands.restore(projectId, targetId));
		// 取り消し用トークンはバックエンドが返さない
		return {
			undoToken: '',
			changedCount: preview.modified.length + preview.deleted.length + preview.created.length
		};
	},

	async undoRestore(): Promise<void> {
		unsupported('undo restore');
	},

	async fetch(projectId: string) {
		return mapPull(await unwrap(commands.pull(projectId)));
	},

	async push(projectId: string): Promise<void> {
		await unwrap(commands.push(projectId));
	},

	async listConflicts(projectId: string): Promise<ConflictFile[]> {
		const items = await unwrap(commands.listConflicts(projectId));
		return items.map(mapConflict);
	},

	async resolveConflicts(projectId, resolutions): Promise<void> {
		const { choices, keepOtherCopy } = mapResolutions(resolutions);
		await unwrap(commands.resolveConflicts(projectId, choices, keepOtherCopy));
	},

	async abortMerge(): Promise<void> {
		unsupported('abort merge');
	},

	async addFiles(): Promise<number> {
		return unsupported('add files');
	},

	async openFile(projectId: string, path: string, target: OpenTarget): Promise<void> {
		if (target !== 'default') unsupported(`open file with target ${target}`);
		const infos = await unwrap(commands.listProjects());
		const project = infos.find((item) => item.id === projectId);
		if (!project) throw new Error(`project not found: ${projectId}`);
		// プロジェクト外へのパス（`..` や絶対パス）は開かない
		const absolute = resolveInProject(project.path, path);
		if (absolute === null) throw new Error(`path is outside the project: ${path}`);
		await openPath(absolute);
	}
};
