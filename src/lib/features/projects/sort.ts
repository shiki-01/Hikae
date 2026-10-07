import type { Project } from '#lib/api/types.js';
import { needsAttention } from '#lib/components/project-card.js';

export function sortProjects(projects: Project[]): Project[] {
	return [...projects].sort((a, b) => {
		const attention = Number(needsAttention(b)) - Number(needsAttention(a));
		if (attention !== 0) return attention;
		return (b.lastSavedAt?.getTime() ?? 0) - (a.lastSavedAt?.getTime() ?? 0);
	});
}
