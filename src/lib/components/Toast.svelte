<script lang="ts" module>
	export type ToastType = 'info' | 'success' | 'warning' | 'error';
</script>

<script lang="ts">
	import { AlertTriangle, CheckCircle2, Info, X, XCircle } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from './Button.svelte';
	import IconButton from './IconButton.svelte';

	interface Props {
		type?: ToastType;
		message: string;
		actionLabel?: string;
		onaction?: () => void;
		onclose?: () => void;
		class?: string;
	}

	let {
		type = 'info',
		message,
		actionLabel,
		onaction,
		onclose,
		class: className = ''
	}: Props = $props();

	const looks = {
		info: { icon: Info, color: 'fg:state-sync', border: 'bl:4px|solid|state-sync' },
		success: { icon: CheckCircle2, color: 'fg:state-saved', border: 'bl:4px|solid|state-saved' },
		warning: {
			icon: AlertTriangle,
			color: 'fg:state-unsaved',
			border: 'bl:4px|solid|state-unsaved'
		},
		error: { icon: XCircle, color: 'fg:state-danger', border: 'bl:4px|solid|state-danger' }
	};

	const look = $derived(looks[type]);
	const Icon = $derived(look.icon);
</script>

<div
	role={type === 'error' || type === 'warning' ? 'alert' : 'status'}
	class={`flex align-items:center gap:3 pl:3 pr:2 py:2 r:md bg:bg-raised fg:fg b:1px|solid|border shadow:overlay ${look.border} ${className}`}
>
	<Icon size={20} class={`${look.color} flex-shrink:0`} aria-hidden="true" />
	<p class="m:0 flex:1 type-body">
		<span class="sr-only">{t(`toast.type.${type}`)}</span>
		{message}
	</p>
	{#if actionLabel}
		<Button size="sm" variant="secondary" onclick={onaction}>{actionLabel}</Button>
	{/if}
	{#if onclose}
		<IconButton label={t('a11y.close')} onclick={onclose}>
			<X size={16} aria-hidden="true" />
		</IconButton>
	{/if}
</div>
