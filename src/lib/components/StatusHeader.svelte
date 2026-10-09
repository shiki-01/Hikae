<script lang="ts">
	import {
		AlertTriangle,
		ArrowLeft,
		CheckCircle2,
		Cloud,
		CloudDownload,
		CloudOff,
		CloudUpload,
		LoaderCircle,
		Pencil,
		Settings
	} from '@lucide/svelte';
	import type { Component } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';
	import Button from './Button.svelte';
	import IconButton from './IconButton.svelte';
	import ProgressBar from './ProgressBar.svelte';
	import { resolveStatus, showsSecondaryFetch, type StatusState } from './status-header';

	interface Props {
		projectName: string;
		hasConflict?: boolean;
		syncing?: 'fetch' | 'push' | 'other' | null;
		/** ユーザーの対応が必要な理由（ぶつかり以外） */
		attention?: 'auth' | 'unsaved-changes' | null;
		/** 前回のアプリ終了で途中で止まった操作が残っている */
		interrupted?: boolean;
		/** 大きいファイルがあるため、自動の取り込みを見送った */
		largeFiles?: boolean;
		/** クラウドの保管場所（GitHub）に接続していない。差分も未保存も無いときに、接続を促す */
		notConnected?: boolean;
		isOnline?: boolean;
		unsavedCount?: number;
		uploadPendingCount?: number;
		fetchPendingCount?: number;
		lastUploadedAt?: Date | null;
		onback?: () => void;
		onsettings?: () => void;
		onfetch?: () => void;
		onpush?: () => void;
		onretry?: () => void;
		onreview?: () => void;
		oninterrupted?: () => void;
		onlargefiles?: () => void;
		onconnect?: () => void;
		/** 再ログインが必要なとき、ログインし直す手順を開く */
		onrelogin?: () => void;
		class?: string;
	}

	let {
		projectName,
		hasConflict = false,
		syncing = null,
		attention = null,
		interrupted = false,
		largeFiles = false,
		notConnected = false,
		isOnline = true,
		unsavedCount = 0,
		uploadPendingCount = 0,
		fetchPendingCount = 0,
		lastUploadedAt = null,
		onback,
		onsettings,
		onfetch,
		onpush,
		onretry,
		onreview,
		oninterrupted,
		onlargefiles,
		onconnect,
		onrelogin,
		class: className = ''
	}: Props = $props();

	interface Display {
		icon: Component;
		color: string;
		text: string;
		action?: { label: string; run?: () => void; variant: 'primary' | 'ghost' };
	}

	const status = $derived(
		resolveStatus(
			hasConflict,
			syncing !== null,
			isOnline,
			unsavedCount,
			fetchPendingCount,
			uploadPendingCount,
			attention !== null,
			interrupted,
			largeFiles,
			notConnected
		)
	);

	const displays: Record<StatusState, Display> = $derived({
		conflict: {
			icon: AlertTriangle,
			color: 'fg:state-danger',
			text: t('header.status_conflict'),
			action: { label: t('header.status_conflict_resolve'), run: onreview, variant: 'primary' }
		},
		interrupted: {
			icon: AlertTriangle,
			color: 'fg:state-danger',
			text: t('header.status_interrupted'),
			action: {
				label: t('header.status_interrupted_review'),
				run: oninterrupted,
				variant: 'primary'
			}
		},
		large_files: {
			icon: AlertTriangle,
			color: 'fg:state-danger',
			text: t('header.status_large_files'),
			action: {
				label: t('header.status_large_files_review'),
				run: onlargefiles,
				variant: 'primary'
			}
		},
		syncing: {
			icon: LoaderCircle,
			color: 'fg:state-sync',
			text:
				syncing === 'push'
					? t('header.status_syncing_push')
					: syncing === 'other'
						? t('header.status_syncing_other')
						: t('header.status_syncing_fetch')
		},
		attention: {
			icon: AlertTriangle,
			color: 'fg:state-unsaved',
			text:
				attention === 'auth'
					? t('header.status_attention_auth')
					: t('header.status_attention_unsaved'),
			// 再ログインが必要なときだけ、ログインし直すボタンを出す
			action:
				attention === 'auth'
					? { label: t('header.relogin'), run: onrelogin, variant: 'primary' }
					: undefined
		},
		offline: {
			icon: CloudOff,
			color: 'fg:fg-muted',
			text: t('header.status_offline'),
			action: { label: t('header.status_offline_retry'), run: onretry, variant: 'ghost' }
		},
		unsaved: {
			icon: Pencil,
			color: 'fg:state-unsaved',
			text: t('header.status_unsaved', { count: unsavedCount })
		},
		fetch_pending: {
			icon: CloudDownload,
			color: 'fg:state-sync',
			text: t('header.status_fetch_pending', { count: fetchPendingCount }),
			action: { label: t('header.fetch'), run: onfetch, variant: 'primary' }
		},
		push_pending: {
			icon: CloudUpload,
			color: 'fg:state-sync',
			text: t('header.status_push_pending', { count: uploadPendingCount }),
			action: { label: t('header.push'), run: onpush, variant: 'primary' }
		},
		not_connected: {
			icon: Cloud,
			color: 'fg:fg-muted',
			text: t('header.status_not_connected'),
			action: { label: t('header.connect'), run: onconnect, variant: 'primary' }
		},
		saved: {
			icon: CheckCircle2,
			color: 'fg:state-saved',
			text: lastUploadedAt
				? t('header.status_saved_time', { time: formatDateTime(lastUploadedAt) })
				: t('header.status_saved'),
			action: { label: t('header.fetch'), run: onfetch, variant: 'ghost' }
		}
	});

	const display = $derived(displays[status]);
	// 未保存の変更が優先されていても、別の PC の変更があれば、控えめな「取り込む」を状態表示とは別に出す
	const secondaryFetch = $derived(showsSecondaryFetch(status, fetchPendingCount, attention));
	const Icon = $derived(display.icon);
</script>

<header
	class={`flex align-items:center justify-content:space-between gap:4 h:56px px:4 flex-shrink:0 bg:bg-subtle bb:1px|solid|border ${className}`}
>
	<div class="flex align-items:center gap:2 min-w:0">
		<IconButton label={t('header.back')} onclick={onback}>
			<ArrowLeft size={20} aria-hidden="true" />
		</IconButton>
		<h1 class="m:0 type-title overflow:hidden text-overflow:ellipsis white-space:nowrap">
			{projectName}
		</h1>
	</div>

	<div class="flex align-items:center gap:3 min-w:0">
		<p
			class="m:0 flex align-items:center gap:2 min-w:0 type-body"
			aria-live="polite"
			data-status={status}
		>
			<Icon
				size={16}
				class={`${display.color} flex-shrink:0 ${status === 'syncing' ? 'motion-spin' : ''}`}
				aria-hidden="true"
			/>
			<span class="overflow:hidden text-overflow:ellipsis white-space:nowrap">{display.text}</span>
		</p>
		{#if secondaryFetch}
			<Button size="sm" variant="ghost" onclick={onfetch}>{t('header.fetch')}</Button>
		{/if}
		{#if status === 'syncing'}
			<div class="w:96px flex-shrink:0">
				<ProgressBar indeterminate label={display.text} />
			</div>
		{:else if display.action}
			<Button size="sm" variant={display.action.variant} onclick={display.action.run}>
				{display.action.label}
			</Button>
		{/if}
		<IconButton label={t('header.settings')} onclick={onsettings}>
			<Settings size={20} aria-hidden="true" />
		</IconButton>
	</div>
</header>
