<script lang="ts">
	import { t, type MessageKey } from '#lib/i18n/index.js';
	import { formatBytes } from '#lib/i18n/format.js';
	import { mobileApi } from '#lib/api/mobile-index.js';
	import type { OnExists, UploadResult } from '#lib/api/mobile-types.js';
	import Button from '#lib/components/Button.svelte';
	import Radio from '#lib/components/Radio.svelte';
	import ProgressBar from '#lib/components/ProgressBar.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import TextField from '#lib/components/TextField.svelte';
	import { reportError, pushToast } from '#lib/features/notifications/store.svelte.js';
	import { fileToBase64, textToBase64 } from './base64';
	import { checkAddable, type LimitProblem } from '#lib/features/mobile/limits.js';
	import { extensionOfImageMime } from './mime';
	import { clipboardFileName, memoFileName } from './names';
	import { readClipboard } from './clipboard';
	import {
		conflictItems,
		hasSaveTargets,
		nameProblem,
		planInputs,
		uploadInputs,
		type NameProblem,
		type NewUploadItem,
		type UploadSource
	} from './upload-items';
	import FolderPickerDialog from './FolderPickerDialog.svelte';
	import ScreenHeader from './ScreenHeader.svelte';
	import {
		addDraftItems,
		applyDraftPlan,
		draft,
		failDraftUpload,
		finishDraftUpload,
		removeDraftItem,
		renameDraftItem,
		resetDraft,
		setDraftFolder,
		setDraftMessage,
		setDraftOnExists,
		startDraftUpload
	} from './upload-draft.svelte';
	import { nav } from './nav.svelte';

	let seq = 0;
	function newId(): string {
		seq += 1;
		return `item-${seq}`;
	}

	/** 処理中の表示（読み込み・確認・保存）。派生値より前に宣言する */
	let busy = $state<'reading' | 'checking' | 'uploading' | null>(null);

	const repo = $derived(nav.repo ?? { owner: '', name: '' });
	const folderLabel = $derived(draft.folder === '' ? t('mobile.files.root') : draft.folder);
	const defaultMessage = $derived(t('mobile.upload.default_message', { n: draft.items.length }));
	const messageValue = $derived(draft.messageDraft ?? defaultMessage);
	const problems = $derived(
		draft.items.map((item) => nameProblem(item.name, draft.items, item.id))
	);
	const submitDisabled = $derived(
		draft.items.length === 0 ||
			problems.some((problem) => problem !== null) ||
			!hasSaveTargets(draft.items) ||
			busy !== null
	);
	const conflictCount = $derived(conflictItems(draft.items).length);

	let limitMessage = $state<string | null>(null);
	let textOpen = $state(false);
	let textDraft = $state('');
	let pasteOpen = $state(false);
	let pasteBuffer = $state('');
	let folderOpen = $state(false);
	let fileInput = $state<HTMLInputElement>();
	let photoInput = $state<HTMLInputElement>();

	const ON_EXISTS: { value: OnExists; label: MessageKey }[] = [
		{ value: 'keepBoth', label: 'mobile.upload.conflict_keep_both' },
		{ value: 'replace', label: 'mobile.upload.conflict_replace' },
		{ value: 'skip', label: 'mobile.upload.conflict_skip' }
	];

	function limitText(problem: LimitProblem): string {
		if (problem.kind === 'file') {
			return t('mobile.upload.limit_file', { name: problem.name, max: formatBytes(problem.limit) });
		}
		if (problem.kind === 'total') {
			return t('mobile.upload.limit_total', { max: formatBytes(problem.limit) });
		}
		return t('mobile.upload.limit_count', { max: problem.limit });
	}

	function problemText(problem: NameProblem | null): string | undefined {
		if (problem === 'empty') return t('mobile.upload.name_empty');
		if (problem === 'invalid') return t('mobile.upload.name_invalid');
		if (problem === 'duplicate') return t('mobile.upload.name_duplicate');
		return undefined;
	}

	/** 上限を確かめてから項目を足す。超えたら何も足さず理由を出す */
	function tryAdd(incoming: { name: string; size: number }[]): boolean {
		const problem = checkAddable(draft.items, incoming);
		if (problem) {
			limitMessage = limitText(problem);
			return false;
		}
		limitMessage = null;
		return true;
	}

	function takenNames(): Set<string> {
		return new Set(draft.items.map((item) => item.name));
	}

	async function addFiles(files: File[], source: UploadSource): Promise<void> {
		if (
			files.length === 0 ||
			!tryAdd(files.map((file) => ({ name: file.name, size: file.size })))
		) {
			return;
		}
		busy = 'reading';
		try {
			const items: NewUploadItem[] = [];
			for (const file of files) {
				items.push({
					id: newId(),
					source,
					name: file.name,
					size: file.size,
					contentBase64: await fileToBase64(file)
				});
			}
			addDraftItems(items);
		} catch (error) {
			reportError(error);
		} finally {
			busy = null;
		}
	}

	function onFilesPicked(event: Event, source: UploadSource): void {
		const input = event.currentTarget as HTMLInputElement;
		const files = Array.from(input.files ?? []);
		// 同じファイルをもう一度選べるように空にする
		input.value = '';
		void addFiles(files, source);
	}

	function addText(text: string, source: UploadSource, name: string): void {
		const size = new TextEncoder().encode(text).length;
		if (!tryAdd([{ name, size }])) return;
		addDraftItems([{ id: newId(), source, name, size, contentBase64: textToBase64(text) }]);
	}

	function addTextItem(): void {
		if (textDraft.trim() === '') return;
		addText(textDraft, 'text', memoFileName(new Date(), takenNames()));
		if (limitMessage === null) textDraft = '';
	}

	async function addClipboardImage(blob: Blob, mime: string): Promise<void> {
		const extension = extensionOfImageMime(mime);
		if (extension === null) {
			pushToast({ type: 'warning', message: t('mobile.upload.clipboard_unsupported') });
			return;
		}
		const name = clipboardFileName(new Date(), extension, takenNames());
		if (!tryAdd([{ name, size: blob.size }])) return;
		busy = 'reading';
		try {
			addDraftItems([
				{
					id: newId(),
					source: 'clipboard',
					name,
					size: blob.size,
					contentBase64: await fileToBase64(blob)
				}
			]);
		} catch (error) {
			reportError(error);
		} finally {
			busy = null;
		}
	}

	function addClipboardText(text: string): void {
		addText(text, 'clipboard', clipboardFileName(new Date(), 'txt', takenNames()));
	}

	async function pasteFromClipboard(): Promise<void> {
		busy = 'reading';
		const content = await readClipboard();
		busy = null;
		if (content.kind === 'unavailable') {
			// 読めなかったときは、貼り付け用の欄から取り込めるようにする
			pasteOpen = true;
			return;
		}
		if (content.kind === 'empty') {
			pushToast({ type: 'info', message: t('mobile.upload.clipboard_empty') });
			return;
		}
		if (content.kind === 'image') await addClipboardImage(content.blob, content.mime);
		else addClipboardText(content.text);
	}

	function onPasteFallback(event: ClipboardEvent): void {
		const data = event.clipboardData;
		if (!data) return;
		const image = Array.from(data.files).find((file) => file.type.startsWith('image/'));
		if (image) {
			event.preventDefault();
			void addClipboardImage(image, image.type);
			return;
		}
		const text = data.getData('text/plain');
		if (text !== '') {
			event.preventDefault();
			addClipboardText(text);
			pasteBuffer = '';
		}
	}

	/** 保存する（先に同名を確かめ、同名があれば 3 択を出して、もう一度押すと保存する） */
	async function submit(): Promise<void> {
		if (submitDisabled) return;
		if (draft.run === 'checked') {
			startDraftUpload();
			await runUpload();
			return;
		}
		busy = 'checking';
		try {
			const plan = await mobileApi.planUpload(
				repo.owner,
				repo.name,
				planInputs(draft.items, draft.folder)
			);
			busy = null;
			const hasConflicts = applyDraftPlan(plan);
			if (hasConflicts) return;
		} catch (error) {
			reportError(error, { retry: () => void submit() });
			return;
		} finally {
			busy = null;
		}
		await runUpload();
	}

	async function runUpload(): Promise<void> {
		const message = messageValue.trim() === '' ? null : messageValue;
		busy = 'uploading';
		try {
			const result: UploadResult = await mobileApi.upload(
				repo.owner,
				repo.name,
				message,
				uploadInputs(draft.items, draft.folder)
			);
			finishDraftUpload(result);
		} catch (error) {
			// 項目は残っているので、編集中へ戻す（データは無事）
			failDraftUpload();
			reportError(error, { retry: () => void submit() });
		} finally {
			busy = null;
		}
	}
</script>

<ScreenHeader title={t('mobile.upload.title')} />

<div class="flex flex-direction:column flex:1 min-h:0 overflow:auto">
	{#if draft.result}
		{@const result = draft.result}
		<section class="flex flex-direction:column gap:3 p:4" aria-live="polite">
			<p class="m:0 type-title">{t('mobile.upload.result_title')}</p>
			<p class="m:0 type-body">{t('mobile.upload.result_saved', { count: result.saved.length })}</p>
			{#if result.skipped.length > 0}
				<p class="m:0 type-body">
					{t('mobile.upload.result_skipped', { count: result.skipped.length })}
				</p>
			{/if}
			{#each result.renamed as rename (rename.to)}
				<p class="m:0 type-small fg:fg-muted">
					{t('mobile.upload.result_renamed', { before: rename.from, after: rename.to })}
				</p>
			{/each}
			<div>
				<Button class="min-h:44px" onclick={() => resetDraft(draft.folder)}
					>{t('mobile.upload.result_reset')}</Button
				>
			</div>
		</section>
	{:else}
		<div class="flex align-items:center gap:2 p:4 bb:1px|solid|border">
			<div class="flex flex-direction:column flex:1 min-w:0">
				<span class="type-small fg:fg-muted">{t('mobile.upload.folder_label')}</span>
				<span class="type-body overflow:hidden text-overflow:ellipsis white-space:nowrap"
					>{folderLabel}</span
				>
			</div>
			<Button
				variant="secondary"
				class="min-h:44px"
				disabled={busy !== null}
				onclick={() => (folderOpen = true)}>{t('mobile.upload.folder_change')}</Button
			>
		</div>

		<div class="flex flex-wrap:wrap gap:2 p:4 bb:1px|solid|border">
			<input
				bind:this={fileInput}
				type="file"
				multiple
				hidden
				onchange={(event) => onFilesPicked(event, 'file')}
			/>
			<input
				bind:this={photoInput}
				type="file"
				accept="image/*"
				multiple
				hidden
				onchange={(event) => onFilesPicked(event, 'photo')}
			/>
			<Button
				variant="secondary"
				class="min-h:44px"
				disabled={busy !== null}
				onclick={() => fileInput?.click()}>{t('mobile.upload.add_file')}</Button
			>
			<Button
				variant="secondary"
				class="min-h:44px"
				disabled={busy !== null}
				onclick={() => photoInput?.click()}>{t('mobile.upload.add_photo')}</Button
			>
			<Button
				variant="secondary"
				class="min-h:44px"
				disabled={busy !== null}
				onclick={() => (textOpen = !textOpen)}>{t('mobile.upload.add_text')}</Button
			>
			<Button
				variant="secondary"
				class="min-h:44px"
				disabled={busy !== null}
				onclick={() => void pasteFromClipboard()}>{t('mobile.upload.add_clipboard')}</Button
			>
		</div>

		{#if textOpen}
			<div class="flex flex-direction:column gap:2 p:4 bb:1px|solid|border">
				<TextField
					ariaLabel={t('mobile.upload.text_placeholder')}
					placeholder={t('mobile.upload.text_placeholder')}
					multiline
					rows={4}
					bind:value={textDraft}
				/>
				<div>
					<Button
						class="min-h:44px"
						disabled={textDraft.trim() === '' || busy !== null}
						onclick={addTextItem}>{t('mobile.upload.text_add')}</Button
					>
				</div>
			</div>
		{/if}

		{#if pasteOpen}
			<div class="flex flex-direction:column gap:2 p:4 bb:1px|solid|border">
				<p class="m:0 type-small fg:fg-muted">{t('mobile.upload.paste_fallback_label')}</p>
				<div
					role="group"
					aria-label={t('mobile.upload.paste_fallback_placeholder')}
					onpaste={onPasteFallback}
				>
					<TextField
						ariaLabel={t('mobile.upload.paste_fallback_placeholder')}
						placeholder={t('mobile.upload.paste_fallback_placeholder')}
						multiline
						rows={2}
						bind:value={pasteBuffer}
						onkeydown={(event) => {
							if (event.key === 'Escape') pasteOpen = false;
						}}
					/>
				</div>
				<div class="flex gap:2">
					<Button variant="ghost" class="min-h:44px" onclick={() => (pasteOpen = false)}
						>{t('mobile.common.close')}</Button
					>
				</div>
			</div>
		{/if}

		{#if limitMessage}
			<p role="alert" class="m:0 p:4 type-small fg:state-danger">{limitMessage}</p>
		{/if}

		<div class="flex flex-direction:column gap:2 p:4">
			<h2 class="m:0 type-heading">{t('mobile.upload.items_title', { n: draft.items.length })}</h2>
			{#if draft.items.length === 0}
				<p class="m:0 type-body fg:fg-muted">{t('mobile.upload.items_empty')}</p>
			{:else}
				<ul class="list-style:none m:0 p:0 flex flex-direction:column gap:2">
					{#each draft.items as item, index (item.id)}
						<li class="flex flex-direction:column gap:2 p:3 r:md b:1px|solid|border">
							<TextField
								ariaLabel={t('mobile.upload.item_name')}
								value={item.name}
								error={problemText(problems[index])}
								disabled={busy !== null}
								oninput={(value) => renameDraftItem(item.id, value)}
							/>
							<div class="flex align-items:center justify-content:space-between gap:2">
								<span class="type-small fg:fg-muted">{formatBytes(item.size)}</span>
								<Button
									variant="ghost"
									size="sm"
									class="min-h:44px"
									disabled={busy !== null}
									onclick={() => removeDraftItem(item.id)}>{t('mobile.upload.item_remove')}</Button
								>
							</div>
							{#if item.exists}
								<fieldset class="m:0 p:0 b:0 flex flex-direction:column gap:2">
									<legend class="type-small font-weight:700"
										>{t('mobile.upload.conflict_title')}</legend
									>
									{#each ON_EXISTS as choice (choice.value)}
										<Radio
											name={`onexists-${item.id}`}
											value={choice.value}
											group={item.onExists}
											label={t(choice.label)}
											onselect={(value) => setDraftOnExists(item.id, value as OnExists)}
										/>
									{/each}
									<p class="m:0 type-small fg:fg-muted">{t('mobile.upload.conflict_note')}</p>
								</fieldset>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
		</div>

		<div class="p:4 bt:1px|solid|border">
			<TextField
				label={t('mobile.upload.message_label')}
				value={messageValue}
				disabled={busy !== null}
				oninput={(value) => setDraftMessage(value)}
			/>
		</div>

		{#if draft.run === 'checked' && conflictCount > 0}
			<p role="status" class="m:0 p:4 type-body fg:state-unsaved">
				{t('mobile.upload.conflict_confirm_hint')}
			</p>
		{/if}

		{#if busy === 'checking'}
			<div class="flex align-items:center gap:2 p:4">
				<Spinner size="sm" />
				<span class="type-small fg:fg-muted">{t('mobile.upload.checking')}</span>
			</div>
		{:else if busy === 'uploading'}
			<div class="flex flex-direction:column gap:2 p:4">
				<ProgressBar indeterminate label={t('mobile.upload.progress')} />
				<span class="type-small fg:fg-muted">{t('mobile.upload.progress')}</span>
			</div>
		{/if}
	{/if}
</div>

{#if !draft.result}
	<div class="flex flex-direction:column gap:2 p:4 bt:1px|solid|border flex-shrink:0">
		<Button class="min-h:44px w:100%" disabled={submitDisabled} onclick={() => void submit()}
			>{t('mobile.upload.submit')}</Button
		>
	</div>
{/if}

{#if folderOpen}
	<FolderPickerDialog
		owner={repo.owner}
		name={repo.name}
		initial={draft.folder}
		onchoose={(path) => {
			setDraftFolder(path);
			folderOpen = false;
		}}
		onclose={() => (folderOpen = false)}
	/>
{/if}
