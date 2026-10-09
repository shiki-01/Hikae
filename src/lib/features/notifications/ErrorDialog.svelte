<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { ChevronDown, ChevronRight, Copy } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { reloginSearch } from '#lib/features/projects/relogin.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { closeError, notifications } from './store.svelte';

	let showDetails = $state(false);
	let copied = $state(false);

	const current = $derived(notifications.error);

	$effect(() => {
		if (!current) {
			showDetails = false;
			copied = false;
		}
	});

	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			copied = false;
		}
	}

	function primary() {
		const state = notifications.error;
		if (!state) return;
		const { view, onretry, onprimary } = state;
		// 管理者向けの説明のコピーは、ダイアログを開いたまま行い、コピーできたことを見せる
		if (view.primaryKind === 'copy' && view.copyText) {
			void copy(view.copyText);
			return;
		}
		closeError();
		if (view.primaryKind === 'retry') onretry?.();
		// ログインし直す画面へ移り、終わったらいまの画面に戻る
		else if (view.primaryKind === 'login')
			void goto(`${resolve('/welcome')}${reloginSearch(page.url)}`);
		onprimary?.();
	}

	function secondary() {
		const state = notifications.error;
		if (!state) return;
		closeError();
		state.onsecondary?.();
	}
</script>

<Dialog
	open={current !== null}
	variant="error"
	title={current?.view.title ?? ''}
	onclose={closeError}
>
	{#if current}
		{@const view = current.view}
		<div class="flex flex-direction:column gap:4">
			<p class="m:0 type-body white-space:pre-line">{view.message}</p>

			<div class="flex flex-direction:column gap:2">
				<button
					type="button"
					aria-expanded={showDetails}
					onclick={() => (showDetails = !showDetails)}
					class="inline-flex align-items:center gap:1 align-self:flex-start p:0 b:0 bg:transparent fg:fg-muted type-small cursor:pointer fg:fg:hover"
				>
					{#if showDetails}
						<ChevronDown size={14} aria-hidden="true" />
						{t('error.hide_details')}
					{:else}
						<ChevronRight size={14} aria-hidden="true" />
						{t('error.show_details')}
					{/if}
				</button>
				{#if showDetails}
					<div class="flex flex-direction:column gap:2 p:3 r:md bg:bg-subtle b:1px|solid|border">
						<pre class="m:0 type-mono white-space:pre-wrap overflow-wrap:anywhere">{view.code ===
								'generic' || view.code === 'backend'
								? ''
								: `${view.code}\n`}{view.technical}</pre>
						<div class="flex align-items:center gap:2 justify-content:end">
							{#if copied}
								<span class="type-small fg:fg-muted" role="status">{t('error.copied')}</span>
							{/if}
							<Button
								size="sm"
								variant="secondary"
								onclick={() =>
									copy(
										view.code === 'generic' || view.code === 'backend'
											? view.technical
											: `${view.code}\n${view.technical}`
									)}
							>
								<Copy size={14} aria-hidden="true" />
								{t('error.copy_details')}
							</Button>
						</div>
					</div>
				{/if}
			</div>
		</div>
	{/if}
	{#snippet actions()}
		{#if current}
			{@const view = current.view}
			{@const closeOnly = view.primaryKind === 'close' && !current.onprimary}
			{#if copied && view.primaryKind === 'copy' && !showDetails}
				<span class="type-small fg:fg-muted align-self:center" role="status">
					{t('error.copied')}
				</span>
			{/if}
			{#if view.secondaryLabel}
				<Button variant="secondary" onclick={secondary}>{view.secondaryLabel}</Button>
			{/if}
			{#if !closeOnly}
				<Button variant="secondary" onclick={closeError}>{t('error.close')}</Button>
			{/if}
			<Button onclick={primary}>
				{#if view.primaryKind === 'copy'}
					<Copy size={14} aria-hidden="true" />
				{/if}
				{closeOnly ? t('error.close') : view.primaryLabel}
			</Button>
		{/if}
	{/snippet}
</Dialog>
