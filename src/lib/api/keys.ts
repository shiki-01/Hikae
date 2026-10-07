export const keys = {
	session: ['session'] as const,
	projects: ['projects'] as const,
	project: (id: string) => ['project', id] as const,
	changes: (id: string) => ['changes', id] as const,
	history: (id: string) => ['history', id] as const,
	pointFiles: (id: string, pointId: string) => ['point-files', id, pointId] as const,
	compare: (id: string, path: string, from: string, to: string) =>
		['compare', id, path, from, to] as const,
	impact: (id: string, target: string, scope: string) => ['impact', id, target, scope] as const,
	conflicts: (id: string) => ['conflicts', id] as const,
	owners: ['owners'] as const,
	remoteProjects: (query: string) => ['remote-projects', query] as const,
	settings: ['settings'] as const
};
