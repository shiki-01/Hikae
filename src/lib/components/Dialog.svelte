<script lang="ts">
	import type { Snippet } from 'svelte';
	import { AlertTriangle, X, XCircle } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import IconButton from './IconButton.svelte';

	interface Props {
		open: boolean;
		title: string;
		description?: string;
		variant?: 'confirm' | 'warning' | 'error' | 'form' | 'fullscreen';
		busy?: boolean;
		hideClose?: boolean;
		onclose: () => void;
		children?: Snippet;
		actions?: Snippet;
		footerNote?: Snippet;
		headerExtra?: Snippet;
	}

	let {
		open,
		title,
		description,
		variant = 'confirm',
		busy = false,
		hideClose = false,
		onclose,
		children,
		actions,
		footerNote,
		headerExtra
	}: Props = $props();

	const id = $props.id();
	let dialog = $state<HTMLDialogElement>();
	let body = $state<HTMLDivElement>();
	let footer = $state<HTMLElement>();

	$effect(() => {
		const el = dialog;
		if (!el) return;
		if (open && !el.open) {
			el.showModal();
			focusInitial();
		} else if (!open && el.open) el.close();
	});

	// 処理中に開いたダイアログは、操作部品がすべて無効のため初期フォーカスを置けない。
	// 処理が終わったときに、フォーカスがダイアログ自体にあるままなら最初の操作部品へ移す
	$effect(() => {
		const el = dialog;
		if (!el || !open || busy) return;
		const active = document.activeElement;
		if (
			el.open &&
			!(active instanceof HTMLElement && el.contains(active) && active.matches(FOCUSABLE))
		) {
			focusInitial();
		}
	});

	const FOCUSABLE =
		'input:not([readonly]):not(:disabled), textarea:not([readonly]):not(:disabled), select:not(:disabled), button:not(:disabled), [tabindex="0"]';

	function focusInitial() {
		queueMicrotask(() => {
			(
				body?.querySelector<HTMLElement>(FOCUSABLE) ?? footer?.querySelector<HTMLElement>(FOCUSABLE)
			)?.focus();
		});
	}

	function handleCancel(event: Event) {
		event.preventDefault();
		if (!busy) onclose();
	}

	const frame = $derived(
		variant === 'fullscreen'
			? 'w:100vw h:100vh max-w:100vw max-h:100vh r:0'
			: variant === 'form'
				? 'w:100% max-w:560px r:lg'
				: 'w:100% max-w:480px r:lg'
	);
	const inner = $derived(variant === 'fullscreen' ? 'h:100vh' : 'max-h:85vh');
</script>

<dialog
	bind:this={dialog}
	aria-labelledby={`${id}-title`}
	aria-describedby={description ? `${id}-desc` : undefined}
	aria-busy={busy || undefined}
	oncancel={handleCancel}
	class={`position:relative p:0 b:0 m:auto bg:bg-raised fg:fg shadow:overlay overflow:hidden ${frame}`}
>
	{#if open}
		<div class={`flex flex-direction:column ${inner}`}>
			<header class="flex align-items:start gap:3 px:6 pt:6 pb:4">
				{#if variant === 'warning'}
					<AlertTriangle size={24} class="fg:state-unsaved flex-shrink:0" aria-hidden="true" />
				{:else if variant === 'error'}
					<XCircle size={24} class="fg:state-danger flex-shrink:0" aria-hidden="true" />
				{/if}
				<div class="flex:1 min-w:0">
					<h2 id={`${id}-title`} class="m:0 type-heading" style:text-wrap="balance">{title}</h2>
					{#if description}
						<p id={`${id}-desc`} class="m:0 mt:1 type-body fg:fg-muted" style:text-wrap="balance">
							{description}
						</p>
					{/if}
				</div>
				{@render headerExtra?.()}
				{#if !hideClose}
					<IconButton label={t('a11y.close')} disabled={busy} onclick={onclose}>
						<X size={18} aria-hidden="true" />
					</IconButton>
				{/if}
			</header>

			<div bind:this={body} class="flex:1 min-h:0 overflow-y:auto px:6 pb:4">
				{@render children?.()}
			</div>

			{#if footerNote}
				<div class="px:6 pb:4">
					{@render footerNote()}
				</div>
			{/if}

			{#if actions}
				<footer
					bind:this={footer}
					class="flex justify-content:end align-items:center gap:3 px:6 py:4 bt:1px|solid|border"
				>
					{@render actions()}
				</footer>
			{/if}
		</div>
	{/if}
</dialog>
