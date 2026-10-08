import { open } from '@tauri-apps/plugin-dialog';
import { commands, events } from '#lib/bindings.js';
import { joinPath } from '#lib/utils/path.js';
import {
	mapAddFilesResult,
	mapChange,
	mapConflict,
	mapDeviceFlow,
	mapDiff,
	mapFileEntries,
	mapFileImpact,
	mapError,
	mapImpact,
	mapOwner,
	mapPointChange,
	mapProject,
	mapPull,
	mapRemoteProjects,
	mapResolutions,
	mapSaveResult,
	mapSavePoint,
	mapSession,
	mapSettingsView,
	parseDate,
	restoreFileFailure,
	toBackendPatch,
	toBackendSizeChoice
} from './mappers';
import type {
	AddFilesOutcome,
	AddProjectInput,
	AppSettings,
	Change,
	ClonePhase,
	ConflictFile,
	DroppedFile,
	FileDiff,
	FileEntry,
	ImpactItem,
	OpenTarget,
	PointFile,
	Project,
	ProjectApi,
	RestoreResult,
	RestoreScope,
	SaveOutcome,
	SavePoint,
	SizeChoice
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

/** GitHub から取得する。進行状況のイベントは、呼び出しに渡した ID で絞り込む */
async function cloneProject(
	input: AddProjectInput,
	onProgress?: (phase: ClonePhase) => void
): Promise<Project> {
	if (!input.remoteId) throw new Error('remote project is not selected');
	const id = crypto.randomUUID();
	const unlisten = await events.cloneProgress.listen((event) => {
		if (event.payload.project_id === id) onProgress?.(event.payload.phase);
	});
	try {
		// 取得先は、選んだフォルダの中に新しく作るフォルダ（存在しないか空である必要がある）
		const info = await unwrap(
			commands.cloneProject(id, input.remoteId, joinPath(input.folder, input.name), input.name)
		);
		return await buildProject(info);
	} finally {
		unlisten();
	}
}

export const tauriApi: ProjectApi = {
	capabilities: { restoreFile: true },

	async getSession() {
		return mapSession(await unwrap(commands.getSession()));
	},

	async startLogin() {
		return mapDeviceFlow(await unwrap(commands.startLogin()));
	},

	waitLogin: () => unwrap(commands.waitLogin()),

	async cancelLogin() {
		await commands.cancelLogin();
	},

	async openLoginPage() {
		await unwrap(commands.openLoginPage());
	},

	async completeOnboarding() {
		await unwrap(commands.completeOnboarding());
	},

	async listOwners() {
		return (await unwrap(commands.listOwners())).map(mapOwner);
	},

	async listRemoteProjects(query: string) {
		return mapRemoteProjects(await unwrap(commands.listRemoteProjects(query)));
	},

	async getSettings(projectId: string | null) {
		return mapSettingsView(await unwrap(commands.getSettings(projectId)));
	},

	async updateSettings(projectId: string | null, patch: Partial<AppSettings>) {
		return mapSettingsView(await unwrap(commands.updateSettings(projectId, toBackendPatch(patch))));
	},

	async listProjects(): Promise<Project[]> {
		const infos = await unwrap(commands.listProjects());
		return Promise.all(infos.map(buildProject));
	},

	getProject: loadProject,

	async pickFolder(): Promise<string | null> {
		const selected = await open({ directory: true, multiple: false });
		return typeof selected === 'string' ? selected : null;
	},

	async pickFiles(): Promise<string[] | null> {
		const selected = await open({ directory: false, multiple: true });
		if (selected === null) return null;
		return Array.isArray(selected) ? selected : [selected];
	},

	async addProject(
		input: AddProjectInput,
		onProgress?: (phase: ClonePhase) => void
	): Promise<Project> {
		if (input.mode === 'github') return cloneProject(input, onProgress);
		const id = crypto.randomUUID();
		await unwrap(commands.addProject(id, input.name, input.folder, input.ownerId, null));
		return loadProject(id);
	},

	async removeProject(id: string): Promise<void> {
		await unwrap(commands.removeProject(id));
	},

	async relocateProject(id: string, folder: string): Promise<Project> {
		return buildProject(await unwrap(commands.relocateProject(id, folder)));
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

	async listPointFiles(projectId: string, savePointId: string): Promise<PointFile[]> {
		const items = await unwrap(commands.listPointChanges(projectId, savePointId));
		return items.map(mapPointChange);
	},

	async listFilesAt(projectId: string, savePointId: string): Promise<FileEntry[]> {
		return mapFileEntries(await unwrap(commands.listFilesAt(projectId, savePointId)));
	},

	suggestMemo: (projectId: string) => unwrap(commands.suggestMemo(projectId)),

	async compare(projectId: string, path: string, fromId: string, toId: string): Promise<FileDiff> {
		const lines = await unwrap(commands.diff(projectId, fromId, toId, path));
		return mapDiff(lines);
	},

	async save(projectId: string, memo: string): Promise<SaveOutcome> {
		return mapSaveResult(await unwrap(commands.save(projectId, memo)));
	},

	async saveWithSizeChoice(
		projectId: string,
		memo: string,
		choice: SizeChoice
	): Promise<SaveOutcome> {
		return mapSaveResult(
			await unwrap(commands.saveWithSizeChoice(projectId, memo, toBackendSizeChoice(choice)))
		);
	},

	async restoreImpact(
		projectId: string,
		targetId: string,
		scope: RestoreScope
	): Promise<ImpactItem[]> {
		if (scope.kind === 'file') {
			const preview = await unwrap(commands.restoreFilePreview(projectId, targetId, scope.path));
			return mapFileImpact(scope.path, preview);
		}
		return mapImpact(await unwrap(commands.restorePreview(projectId, targetId)));
	},

	async restore(projectId: string, targetId: string, scope: RestoreScope): Promise<RestoreResult> {
		if (scope.kind === 'file') {
			const result = await unwrap(commands.restoreFile(projectId, targetId, scope.path));
			if (result.outcome !== 'restored') throw restoreFileFailure(result.outcome, scope.path);
			return { undoToken: result.undo_token, changedCount: 1 };
		}
		const preview = await unwrap(commands.restorePreview(projectId, targetId));
		const result = await unwrap(commands.restore(projectId, targetId));
		return {
			undoToken: result.undo_token,
			changedCount: preview.modified.length + preview.deleted.length + preview.created.length
		};
	},

	async undoRestore(projectId: string, undoToken: string): Promise<void> {
		await unwrap(commands.undoRestore(projectId, undoToken));
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

	async abortMerge(projectId: string): Promise<void> {
		await unwrap(commands.abortMerge(projectId));
	},

	async addFiles(projectId: string, files: DroppedFile[]): Promise<AddFilesOutcome> {
		const paths = files.map((file) => file.path);
		if (paths.some((path) => path === undefined)) unsupported('adding files without a real path');
		// プロジェクト直下へコピーする。サイズの検査と別名化はバックエンドが行う
		const result = await unwrap(commands.addFiles(projectId, paths as string[], ''));
		return mapAddFilesResult(result);
	},

	async openFile(projectId: string, path: string, target: OpenTarget): Promise<void> {
		if (target !== 'default') unsupported(`open file with target ${target}`);
		// パスの検証（プロジェクト外の拒否）はバックエンド側で行う
		await unwrap(commands.openProjectFile(projectId, path));
	}
};
