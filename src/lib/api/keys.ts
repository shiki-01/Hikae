export const keys = {
	session: ['session'] as const,
	/** アプリ全体の健全性（保存に必要な部品が使えるか） */
	appHealth: ['app-health'] as const,
	projects: ['projects'] as const,
	project: (id: string) => ['project', id] as const,
	changes: (id: string) => ['changes', id] as const,
	history: (id: string) => ['history', id] as const,
	pointFiles: (id: string, pointId: string) => ['point-files', id, pointId] as const,
	filesAt: (id: string, pointId: string) => ['files-at', id, pointId] as const,
	/** 「すべてのファイル」の一覧 */
	projectTree: (id: string) => ['project-tree', id] as const,
	memoSuggestion: (id: string) => ['memo-suggestion', id] as const,
	compare: (id: string, path: string, from: string, to: string) =>
		['compare', id, path, from, to] as const,
	impact: (id: string, target: string, scope: string) => ['impact', id, target, scope] as const,
	conflicts: (id: string) => ['conflicts', id] as const,
	owners: ['owners'] as const,
	remoteProjects: (query: string) => ['remote-projects', query] as const,
	/** プロジェクトを指定しない場合は全体設定 */
	settings: (projectId: string | null) => ['settings', projectId ?? 'global'] as const,
	settingsAll: ['settings'] as const
};
