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
	/** AI（ローカル LLM）のモデル一覧。ダウンロードの終了で取り直す */
	aiCatalog: ['ai-catalog'] as const,
	/** 保存メモの AI 提案。変更一覧と設定（モデル・範囲）の組み合わせごとに別の結果 */
	aiSuggestion: (id: string, key: string) => ['ai-suggestion', id, key] as const,
	/** 保存しないファイル（`.gitignore`）のパターン一覧 */
	ignoreRules: (id: string) => ['ignore-rules', id] as const,
	/** 追跡中で、保存しないファイルに当たるファイル */
	trackedIgnored: (id: string) => ['tracked-ignored', id] as const,
	/** 「今後の変更だけ無視する」の記録と、この PC での現在の状態 */
	skipWorktree: (id: string) => ['skip-worktree', id] as const,
	/** 無視が外れている記録（再適用の提案の対象） */
	skipDrift: (id: string) => ['skip-drift', id] as const,
	compare: (id: string, path: string, from: string, to: string) =>
		['compare', id, path, from, to] as const,
	/** Office 文書の比較。`['compare', id]` の接頭辞で、変更後に一緒に無効化される */
	compareOffice: (id: string, path: string, from: string, to: string) =>
		['compare', id, 'office', path, from, to] as const,
	/** ファイルの内容の表示（プレビュー）。`['compare', id]` の接頭辞で、変更後に一緒に無効化される */
	filePreview: (id: string, rev: string, path: string) =>
		['compare', id, 'preview', rev, path] as const,
	/** 表示拡張の一覧（起動中は変わらない） */
	renderers: ['renderers'] as const,
	/** 表示に使う画像・pdf の中身（app-blob の URL ごと） */
	mediaBytes: (url: string) => ['media-bytes', url] as const,
	/** 比較に使う画像の中身（app-blob の URL ごと。404 とその他の失敗を区別して返す） */
	compareMedia: (url: string) => ['compare-media', url] as const,
	impact: (id: string, target: string, scope: string) => ['impact', id, target, scope] as const,
	conflicts: (id: string) => ['conflicts', id] as const,
	/** 行ごとの解消で開く、競合中のファイルの全文 */
	conflictText: (id: string, path: string) => ['conflict-text', id, path] as const,
	owners: ['owners'] as const,
	remoteProjects: (query: string) => ['remote-projects', query] as const,
	/** プロジェクトを指定しない場合は全体設定 */
	settings: (projectId: string | null) => ['settings', projectId ?? 'global'] as const,
	settingsAll: ['settings'] as const
};
