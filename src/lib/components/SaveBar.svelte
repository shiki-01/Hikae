<script lang="ts">
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import { t } from '#lib/i18n/index.js';
	import {
		suggestFailureKey,
		suggestNoticeKey,
		type SuggestState
	} from '#lib/features/ai/suggest.js';
	import {
		DETAIL_MAX_CHARS,
		DETAIL_MAX_LINES,
		detailMeter
	} from '#lib/features/changes/memo-draft.js';
	import type { SensitiveKind } from '#lib/api/types.js';
	import Button from './Button.svelte';
	import Dialog from './Dialog.svelte';
	import Spinner from './Spinner.svelte';
	import TextField from './TextField.svelte';

	interface Props {
		/** 要約（1 行）。保存のメモの 1 行目になる */
		memo: string;
		/** 詳細（任意の複数行）。要約の後に空行を挟んで保存される */
		detail: string;
		/** 詳細の欄を開いているか */
		detailOpen?: boolean;
		hasChanges: boolean;
		saving?: boolean;
		/** AI の提案を作れるか（機能が使え、変更がある）。false のときは提案の欄を出さない */
		canSuggest?: boolean;
		/** AI の提案の画面の状態。押したときだけ生成し、結果は「使う」で下書きに入る */
		suggestState?: SuggestState;
		/** 保存欄のメモに見える機微情報の種類（内容は含まない）。空でなければ保存前に確認する */
		sensitive?: SensitiveKind[];
		/** 直前の下書きに戻せるか（提案を使った直後）。 */
		undoable?: boolean;
		onsave: () => void;
		onrequest?: () => void;
		onregenerate?: () => void;
		oncancel?: () => void;
		onusesuggestion?: (suggestion: { title: string; body: string | null }) => void;
		onundosuggestion?: () => void;
		class?: string;
	}

	let {
		memo = $bindable(),
		detail = $bindable(),
		detailOpen = $bindable(false),
		hasChanges,
		saving = false,
		canSuggest = false,
		suggestState = { kind: 'idle' },
		sensitive = [],
		undoable = false,
		onsave,
		onrequest,
		onregenerate,
		oncancel,
		onusesuggestion,
		onundosuggestion,
		class: className = ''
	}: Props = $props();

	const detailId = $props.id();
	const meter = $derived(detailMeter(detail));
	let confirmOpen = $state(false);

	/** 保存のボタン。機微に見える文字列があれば、確認のダイアログを先に出す */
	function requestSave() {
		if (sensitive.length > 0) {
			confirmOpen = true;
			return;
		}
		onsave();
	}
</script>

<section
	aria-label={t('save_bar.label')}
	class={`flex flex-direction:column gap:2 px:4 py:3 flex-shrink:0 bg:bg-subtle bt:1px|solid|border ${className}`}
>
	<div class="flex align-items:end gap:4">
		<TextField
			bind:value={memo}
			multiline
			rows={1}
			maxRows={2}
			label={t('save_bar.memo')}
			placeholder={hasChanges ? t('save_bar.placeholder') : t('save_bar.empty')}
			disabled={!hasChanges || saving}
			class="flex:1"
		/>
		<div class="flex align-items:center gap:3 flex-shrink:0">
			{#if canSuggest && hasChanges}
				<div class="flex align-items:center gap:2" aria-live="polite">
					{#if suggestState.kind === 'idle'}
						<Button variant="ghost" size="sm" onclick={() => onrequest?.()}>
							<Sparkles size={14} aria-hidden="true" />
							{t('save_bar.ai_request')}
						</Button>
					{:else if suggestState.kind === 'generating'}
						<span class="flex align-items:center gap:2 type-small fg:fg-muted">
							<Spinner size="sm" label={t('save_bar.ai_generating')} />
							{t('save_bar.ai_generating')}
						</span>
						<Button variant="ghost" size="sm" onclick={() => oncancel?.()}>
							{t('save_bar.ai_cancel')}
						</Button>
					{:else if suggestState.kind === 'result'}
						<span
							class="max-w:240px overflow:hidden text-overflow:ellipsis white-space:nowrap type-small"
							title={suggestState.title}
						>
							{suggestState.title}
						</span>
						<Button
							variant="ghost"
							size="sm"
							disabled={saving}
							ariaLabel={t('save_bar.ai_use')}
							onclick={() =>
								onusesuggestion?.({
									title: suggestState.kind === 'result' ? suggestState.title : '',
									body: suggestState.kind === 'result' ? suggestState.body : null
								})}
						>
							{t('save_bar.ai_use_button')}
						</Button>
						<Button variant="ghost" size="sm" onclick={() => onregenerate?.()}>
							{t('save_bar.ai_regenerate')}
						</Button>
					{:else}
						<span class="max-w:320px type-small fg:fg-muted">
							{t(suggestFailureKey(suggestState.failure))}
						</span>
						<Button variant="ghost" size="sm" onclick={() => onrequest?.()}>
							{t('save_bar.ai_retry')}
						</Button>
					{/if}
					{#if undoable}
						<Button
							variant="ghost"
							size="sm"
							disabled={saving}
							onclick={() => onundosuggestion?.()}
						>
							{t('save_bar.ai_undo')}
						</Button>
					{/if}
				</div>
			{/if}
			<Button disabled={!hasChanges} loading={saving} onclick={requestSave}>
				{saving ? t('save_bar.saving') : t('save_bar.button')}
			</Button>
		</div>
	</div>

	<div class="flex flex-direction:column gap:1">
		{#if detailOpen}
			<div id={detailId} class="flex flex-direction:column gap:1">
				<TextField
					bind:value={detail}
					multiline
					rows={3}
					maxRows={8}
					label={t('save_bar.detail')}
					placeholder={t('save_bar.detail_placeholder')}
					disabled={!hasChanges || saving}
					class="w:100%"
				/>
				<div class="flex align-items:center justify-content:space-between gap:3">
					<span class={`type-small ${meter.over ? 'fg:state-danger' : 'fg:fg-muted'}`}>
						{t('save_bar.detail_meter', {
							chars: meter.chars,
							max_chars: DETAIL_MAX_CHARS,
							lines: meter.lines,
							max_lines: DETAIL_MAX_LINES
						})}
					</span>
					<Button
						variant="ghost"
						size="sm"
						ariaExpanded={true}
						ariaControls={detailId}
						onclick={() => (detailOpen = false)}
					>
						{t('save_bar.detail_close')}
					</Button>
				</div>
			</div>
		{:else}
			<div>
				<Button
					variant="ghost"
					size="sm"
					disabled={!hasChanges || saving}
					ariaExpanded={false}
					onclick={() => (detailOpen = true)}
				>
					{t('save_bar.detail_open')}
				</Button>
			</div>
		{/if}
	</div>

	<p class="m:0 type-small fg:fg-muted">{t('save_bar.note')}</p>

	{#if sensitive.length > 0}
		<p role="alert" class="m:0 flex align-items:center gap:2 type-small fg:state-danger">
			<AlertTriangle size={14} aria-hidden="true" />
			{t('save_bar.sensitive_warning')}
		</p>
	{/if}

	{#if suggestState.kind === 'result' && suggestState.notices.length > 0}
		<ul class="m:0 p:0 list-style:none flex flex-direction:column gap:1">
			{#each suggestState.notices as notice (notice)}
				<li class="type-small fg:fg-muted">{t(suggestNoticeKey(notice))}</li>
			{/each}
		</ul>
	{/if}
</section>

<Dialog
	open={confirmOpen}
	title={t('save_bar.sensitive_title')}
	variant="warning"
	onclose={() => (confirmOpen = false)}
>
	<p class="m:0 type-body">{t('save_bar.sensitive_warning')}</p>
	{#snippet actions()}
		<Button variant="primary" onclick={() => (confirmOpen = false)}>
			{t('save_bar.sensitive_back')}
		</Button>
		<Button
			variant="secondary"
			onclick={() => {
				confirmOpen = false;
				onsave();
			}}
		>
			{t('save_bar.sensitive_confirm')}
		</Button>
	{/snippet}
</Dialog>
