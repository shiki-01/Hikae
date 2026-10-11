<script lang="ts">
	import { useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/index.js';
	import { keys } from '#lib/api/keys.js';
	import type {
		AiCustomGgufCheck,
		AiDownloadNotice,
		AiEndpointCheck,
		AiModelSource,
		AiRunnerMode,
		AiScopeMode,
		AiTestFailure,
		AiTestResult,
		AppSettings
	} from '#lib/api/types.js';
	import { t, type MessageKey } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import ModelCard from '#lib/components/ModelCard.svelte';
	import Select from '#lib/components/Select.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import TextField from '#lib/components/TextField.svelte';
	import Toggle from '#lib/components/Toggle.svelte';
	import { useSettings, useUpdateSettings } from '#lib/features/settings/queries.js';
	import { reportError } from '#lib/features/notifications/store.svelte.js';
	import { customGgufMessageKey, formatBytes, scopeHintKey, type ModelAction } from './model-view';
	import { useAiCatalog } from './queries';

	const catalog = useAiCatalog();
	const settings = useSettings(() => null);
	const update = useUpdateSettings(() => null);
	const client = useQueryClient();

	/** ダウンロード中の受信量と、直前の失敗（モデル ID ごと） */
	let progress = $state<Record<string, { received: number; total: number }>>({});
	let failed = $state<Record<string, boolean>>({});
	let test = $state<{ running: boolean; result: AiTestResult | null }>({
		running: false,
		result: null
	});
	let endpoint = $state<{
		checking: boolean;
		result: AiEndpointCheck | null;
		models: string[] | null;
		listFailed: boolean;
	}>({ checking: false, result: null, models: null, listFailed: false });
	/** Ollama の URL の入力中の値。空なら保存された値を表示する */
	let urlDraft = $state<string | null>(null);
	/** 自分で用意した GGUF のパスの入力中の値。空なら保存された値を表示する */
	let pathDraft = $state<string | null>(null);
	/** パスの検査の結果（検査した時点のパスと一緒に持ち、今のパスと違う結果は表示しない） */
	let customCheck = $state<{ path: string; result: AiCustomGgufCheck } | null>(null);

	const current = $derived(settings.data?.settings);
	const urlValue = $derived(urlDraft ?? current?.aiOllamaUrl ?? '');
	const pathValue = $derived(pathDraft ?? current?.aiCustomGgufPath ?? '');
	/** 内蔵で自分で用意した GGUF を使うか */
	const usingCustom = $derived(
		current?.aiRunner === 'builtin' && current?.aiModelSource === 'custom-gguf'
	);
	/** 内蔵のカタログのモデルの段階（選ばれていないとき、Ollama のときは null） */
	const selectedStage = $derived(
		current && current.aiRunner === 'builtin' && current.aiModelSource === 'catalog'
			? (catalog.data?.models.find((m) => m.id === current.aiModel)?.stage ?? null)
			: null
	);
	const scopeHint = $derived(scopeHintKey(selectedStage, current?.aiScope ?? 'text-diff'));
	/** 保存されているパスの検査の結果（パスが変わった後の古い結果は出さない） */
	const pathResult = $derived(
		customCheck && current && customCheck.path === current.aiCustomGgufPath
			? customCheck.result
			: null
	);

	// 保存されている GGUF のパスを、カスタムの入手元のときに検査する（読み取りのみ）
	$effect(() => {
		const path = usingCustom ? (current?.aiCustomGgufPath ?? '') : '';
		if (path !== '') void checkCustomPath(path);
	});

	// ダウンロードの通知（進捗・終了）を、この画面が開いている間だけ受ける
	$effect(() => api.onAiDownload((notice) => onDownloadNotice(notice)));

	function onDownloadNotice(notice: AiDownloadNotice) {
		if (notice.kind === 'progress') {
			progress = { ...progress, [notice.id]: { received: notice.received, total: notice.total } };
			return;
		}
		const next = { ...progress };
		delete next[notice.id];
		progress = next;
		failed = { ...failed, [notice.id]: notice.outcome === 'failed' };
		void client.invalidateQueries({ queryKey: keys.aiCatalog });
	}

	function patch(values: Partial<AppSettings>) {
		update.mutate(values);
	}

	async function act(id: string, action: ModelAction) {
		try {
			if (action === 'download') {
				failed = { ...failed, [id]: false };
				await api.aiDownloadModel(id);
			} else if (action === 'cancel') {
				await api.aiCancelDownload(id);
			} else if (action === 'remove') {
				await api.aiRemoveModel(id);
				void client.invalidateQueries({ queryKey: keys.aiCatalog });
				void client.invalidateQueries({ queryKey: keys.settingsAll });
			} else {
				// カタログのモデルを選んだときは、入手元もカタログに戻す
				patch({ aiModel: id, aiModelSource: 'catalog' });
			}
		} catch (error) {
			reportError(error);
		}
	}

	/** 自分で用意した GGUF のパスを検査する（読み取りのみ）。結果はパスと一緒に持つ */
	async function checkCustomPath(path: string) {
		try {
			const result = await api.aiCheckCustomGguf(path);
			customCheck = { path, result };
		} catch (error) {
			reportError(error);
		}
	}

	/** 入力中のパスを保存し、検査する。変わっていなければ何もしない */
	function saveCustomPath() {
		const next = (pathDraft ?? '').trim();
		pathDraft = null;
		if (next !== (current?.aiCustomGgufPath ?? '')) {
			patch({ aiCustomGgufPath: next });
			if (next !== '') void checkCustomPath(next);
		}
	}

	function setSource(value: AiModelSource) {
		patch({ aiModelSource: value });
	}

	/** 「選ぶ」: ファイル選択で GGUF を 1 つ選び、保存して検査する */
	async function pickCustom() {
		try {
			const picked = await api.pickGgufFile();
			if (picked === null) return;
			pathDraft = null;
			patch({ aiCustomGgufPath: picked });
			await checkCustomPath(picked);
		} catch (error) {
			reportError(error);
		}
	}

	async function runTest() {
		test = { running: true, result: null };
		try {
			test = { running: false, result: await api.aiTestGeneration() };
		} catch (error) {
			test = { running: false, result: null };
			reportError(error);
		}
	}

	function reasonKey(reason: AiTestFailure | null): MessageKey {
		return reason ? (`ai.reason.${reason}` as MessageKey) : 'ai.reason.other';
	}

	function saveUrl() {
		const next = (urlDraft ?? '').trim();
		urlDraft = null;
		if (next !== '' && next !== current?.aiOllamaUrl) patch({ aiOllamaUrl: next });
	}

	async function checkEndpoint() {
		const url = urlValue.trim();
		endpoint = { checking: true, result: null, models: null, listFailed: false };
		try {
			const result = await api.aiCheckEndpoint(url);
			endpoint = { checking: false, result, models: null, listFailed: false };
		} catch (error) {
			endpoint = { checking: false, result: null, models: null, listFailed: false };
			reportError(error);
		}
	}

	async function loadOllamaModels() {
		try {
			endpoint = {
				...endpoint,
				models: await api.aiListOllamaModels(urlValue.trim()),
				listFailed: false
			};
		} catch {
			endpoint = { ...endpoint, models: null, listFailed: true };
		}
	}

	const scopeOptions = $derived([
		{ value: 'file-names', label: t('ai.scope_file_names') },
		{ value: 'text-diff', label: t('ai.scope_text_diff') }
	]);
	const sourceOptions = $derived([
		{ value: 'catalog', label: t('ai.source_catalog') },
		{ value: 'custom-gguf', label: t('ai.source_custom') }
	]);
	const runnerOptions = $derived([
		{ value: 'builtin', label: t('ai.runner_builtin') },
		{ value: 'ollama', label: t('ai.runner_ollama') }
	]);
	const modelOptions = $derived(
		(endpoint.models ?? []).map((name) => ({ value: name, label: name }))
	);
</script>

<div class="flex flex-direction:column gap:4">
	<p class="m:0 type-body fg:fg-muted">{t('ai.intro')}</p>

	<div class="flex flex-direction:column">
		<div
			class="flex align-items:center justify-content:space-between gap:6 py:3 bb:1px|solid|border"
		>
			<div class="flex flex-direction:column gap:1 min-w:0">
				<span class="type-body font-weight:500">{t('ai.enabled')}</span>
				<span class="type-small fg:fg-muted">{t('ai.enabled_text')}</span>
			</div>
			{#if current}
				<Toggle
					label={t('ai.enabled')}
					checked={current.memoSuggestion === 'ai'}
					onchange={(checked) => patch({ memoSuggestion: checked ? 'ai' : 'rules' })}
				/>
			{/if}
		</div>
		<div
			class="flex align-items:center justify-content:space-between gap:6 py:3 bb:1px|solid|border"
		>
			<div class="flex flex-direction:column gap:1 min-w:0">
				<span class="type-body font-weight:500">{t('ai.scope')}</span>
				<span class="type-small fg:fg-muted">{t('ai.scope_text')}</span>
			</div>
			{#if current}
				<Select
					class="w:200px flex-shrink:0"
					ariaLabel={t('ai.scope')}
					value={current.aiScope}
					options={scopeOptions}
					onchange={(value) => patch({ aiScope: value as AiScopeMode })}
				/>
			{/if}
		</div>
	</div>

	{#if scopeHint}
		<p class="m:0 type-small fg:fg-muted">{t(scopeHint)}</p>
	{/if}

	{#if current && current.memoSuggestion !== 'ai'}
		<p class="m:0 type-body fg:fg-muted">{t('ai.rules_only_note')}</p>
	{/if}

	{#if catalog.data}
		<p class="m:0 type-small fg:fg-muted">
			{t('ai.system', {
				memory: formatBytes(catalog.data.system.totalMemoryBytes),
				disk:
					catalog.data.system.freeDiskBytes > 0
						? formatBytes(catalog.data.system.freeDiskBytes)
						: '-'
			})}
		</p>
	{/if}

	<section class="flex flex-direction:column gap:3" aria-label={t('ai.models_heading')}>
		<h3 class="m:0 type-body font-weight:600">{t('ai.models_heading')}</h3>
		<p class="m:0 type-small fg:fg-muted">{t('ai.models_text')}</p>
		{#if usingCustom}
			<p class="m:0 type-small" role="status">{t('ai.custom_active')}</p>
		{/if}
		{#if catalog.isPending}
			<Skeleton class="h:96px" />
		{:else if catalog.data}
			{#each catalog.data.models as model (model.id)}
				<ModelCard
					{model}
					selected={!usingCustom && current?.aiModel === model.id}
					progress={progress[model.id] ?? null}
					failed={failed[model.id] ?? false}
					onaction={(action) => void act(model.id, action)}
				/>
			{/each}
		{/if}
	</section>

	<div class="flex flex-direction:column gap:2">
		<div class="flex align-items:center gap:3">
			<Button variant="secondary" size="sm" loading={test.running} onclick={() => void runTest()}>
				{test.running ? t('ai.test_running') : t('ai.test')}
			</Button>
		</div>
		{#if test.result}
			<p
				class={`m:0 type-small ${test.result.ok ? 'fg:fg-muted' : 'fg:state-danger'}`}
				role="status"
			>
				{#if test.result.ok}
					{t('ai.test_ok')}
				{:else}
					{t('ai.e19')} {t(reasonKey(test.result.reason))}
				{/if}
			</p>
		{/if}
	</div>

	<details class="b:1px|solid|border r:md p:3">
		<summary class="type-body cursor:pointer">{t('ai.advanced')}</summary>
		{#if current}
			<div class="flex flex-direction:column gap:3 mt:3">
				<div class="flex align-items:center justify-content:space-between gap:6">
					<span class="type-body">{t('ai.runner')}</span>
					<Select
						class="w:240px flex-shrink:0"
						ariaLabel={t('ai.runner')}
						value={current.aiRunner}
						options={runnerOptions}
						onchange={(value) => patch({ aiRunner: value as AiRunnerMode })}
					/>
				</div>

				{#if current.aiRunner === 'builtin'}
					<div class="flex align-items:center justify-content:space-between gap:6">
						<span class="type-body">{t('ai.source')}</span>
						<Select
							class="w:240px flex-shrink:0"
							ariaLabel={t('ai.source')}
							value={current.aiModelSource}
							options={sourceOptions}
							onchange={(value) => setSource(value as AiModelSource)}
						/>
					</div>
					{#if current.aiModelSource === 'custom-gguf'}
						<div class="flex align-items:flex-end gap:3 flex-wrap:wrap">
							<div class="flex-grow:1 min-w:0">
								<TextField
									label={t('ai.custom_path')}
									value={pathValue}
									oninput={(value) => (pathDraft = value)}
									onblur={saveCustomPath}
								/>
							</div>
							<Button variant="secondary" size="sm" onclick={() => void pickCustom()}>
								{t('ai.custom_pick')}
							</Button>
						</div>
						{#if pathResult}
							<p
								class={`m:0 type-small ${pathResult.ok ? 'fg:fg-muted' : 'fg:state-danger'}`}
								role="status"
							>
								{pathResult.ok
									? t('ai.custom_ok')
									: t(customGgufMessageKey(pathResult.reason ?? 'unreadable'))}
							</p>
						{/if}
						<p class="m:0 type-small fg:fg-muted">{t('ai.custom_caution')}</p>
					{/if}
				{/if}

				{#if current.aiRunner === 'ollama'}
					<TextField
						type="url"
						label={t('ai.ollama_url')}
						value={urlValue}
						oninput={(value) => (urlDraft = value)}
						onblur={saveUrl}
					/>
					<div class="flex align-items:center gap:3 flex-wrap:wrap">
						<Button
							variant="secondary"
							size="sm"
							loading={endpoint.checking}
							onclick={() => void checkEndpoint()}
						>
							{t('ai.ollama_check')}
						</Button>
						<Button variant="secondary" size="sm" onclick={() => void loadOllamaModels()}>
							{t('ai.ollama_list')}
						</Button>
					</div>
					{#if endpoint.result}
						<p class="m:0 type-small" role="status">
							{endpoint.result.ok ? t('ai.ollama_ok') : t('ai.ollama_ng')}
						</p>
						{#if endpoint.result.externalHost}
							<p class="m:0 type-small fg:state-danger" role="alert">{t('ai.external_warning')}</p>
						{/if}
					{/if}
					{#if endpoint.listFailed}
						<p class="m:0 type-small fg:state-danger">{t('ai.ollama_list_failed')}</p>
					{/if}
					{#if endpoint.models}
						<Select
							ariaLabel={t('ai.ollama_models')}
							value={current.aiModel}
							options={[{ value: '', label: t('ai.none_selected') }, ...modelOptions]}
							onchange={(value) => patch({ aiModel: value })}
						/>
					{/if}
				{/if}
			</div>
		{/if}
	</details>
</div>
