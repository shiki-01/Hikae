<script lang="ts">
	import { AlertTriangle, ArrowRight, Minus, Pencil, Plus } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { ChangeType } from '#lib/api/types.js';

	interface Props {
		type: ChangeType;
		/** 変更のぶつかり中。種類よりも優先して表示する */
		conflict?: boolean;
		class?: string;
	}

	let { type, conflict = false, class: className = '' }: Props = $props();

	const icons = { modified: Pencil, added: Plus, deleted: Minus, renamed: ArrowRight };
	const colors = {
		modified: 'fg:fg-muted',
		added: 'fg:state-saved',
		deleted: 'fg:state-danger',
		renamed: 'fg:fg-muted'
	};

	const Icon = $derived(conflict ? AlertTriangle : icons[type]);
	const color = $derived(conflict ? 'fg:state-danger font-weight:700' : colors[type]);
</script>

<!-- 変更の種類を、色だけでなくアイコンと文言でも示す小さなバッジ -->
<span
	class={`inline-flex align-items:center gap:1 type-small white-space:nowrap flex-shrink:0 ${color} ${className}`}
>
	<Icon size={12} aria-hidden="true" />
	{conflict ? t('change.conflict') : t(`change.${type}`)}
</span>
