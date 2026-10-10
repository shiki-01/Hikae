// 段階ごとの導入条件の判定（設計 10.2、10.5）。OS の値は呼び出し側が `SystemInfo` として渡す純粋な判定。

use crate::catalog::CatalogEntry;

/// 導入先 PC の状態（呼び出し側が取得した値）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemInfo {
    /// OS が報告する総メモリ（バイト）
    pub total_memory_bytes: u64,
    /// モデルを置くボリュームの空き容量（バイト）
    pub free_disk_bytes: u64,
}

/// 導入を満たせない理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NotEnoughMemory { needed: u64, have: u64 },
    NotEnoughDisk { needed: u64, have: u64 },
}

/// 導入可否。`ok == false` のときは `reason` が必ず入る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Availability {
    pub ok: bool,
    pub reason: Option<Reason>,
}

/// 導入に必要な空き容量。ダウンロードと展開の余裕として、サイズの 1.1 倍とする。
pub fn required_disk_bytes(size_bytes: u64) -> u64 {
    size_bytes.saturating_mul(11) / 10
}

/// モデルを導入できるかを判定する。メモリを先に判定し、足りなければその理由を返す。
pub fn stage_availability(info: SystemInfo, model: &CatalogEntry) -> Availability {
    if info.total_memory_bytes < model.min_total_memory_bytes {
        return Availability {
            ok: false,
            reason: Some(Reason::NotEnoughMemory {
                needed: model.min_total_memory_bytes,
                have: info.total_memory_bytes,
            }),
        };
    }
    let needed = required_disk_bytes(model.size_bytes);
    if info.free_disk_bytes < needed {
        return Availability {
            ok: false,
            reason: Some(Reason::NotEnoughDisk {
                needed,
                have: info.free_disk_bytes,
            }),
        };
    }
    Availability {
        ok: true,
        reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn model(size: u64, min_mem: u64) -> CatalogEntry {
        CatalogEntry {
            id: "m".into(),
            display_name: "m".into(),
            stage: 2,
            file_name: "m.gguf".into(),
            url: "https://huggingface.co/a/resolve/main/m.gguf".into(),
            sha256: "0".repeat(64),
            size_bytes: size,
            license_name: "Apache License 2.0".into(),
            license_url: "https://www.apache.org/licenses/LICENSE-2.0".into(),
            min_total_memory_bytes: min_mem,
            recommended: false,
        }
    }

    #[test]
    fn disk_requirement_is_ten_percent_more() {
        assert_eq!(required_disk_bytes(1000), 1100);
        assert_eq!(required_disk_bytes(0), 0);
    }

    #[test]
    fn ok_when_both_are_enough() {
        let m = model(3_000_000_000, 7 * GIB);
        let info = SystemInfo {
            total_memory_bytes: 8 * GIB,
            free_disk_bytes: 4 * GIB,
        };
        assert_eq!(
            stage_availability(info, &m),
            Availability {
                ok: true,
                reason: None
            }
        );
    }

    #[test]
    fn memory_shortage_is_reported_first() {
        let m = model(3_000_000_000, 7 * GIB);
        let info = SystemInfo {
            total_memory_bytes: 4 * GIB,
            free_disk_bytes: 0,
        };
        let a = stage_availability(info, &m);
        assert!(!a.ok);
        assert_eq!(
            a.reason,
            Some(Reason::NotEnoughMemory {
                needed: 7 * GIB,
                have: 4 * GIB
            })
        );
    }

    #[test]
    fn disk_shortage_uses_one_point_one_times_size() {
        let m = model(1000, 0);
        let info = SystemInfo {
            total_memory_bytes: 0,
            free_disk_bytes: 1099,
        };
        assert_eq!(
            stage_availability(info, &m).reason,
            Some(Reason::NotEnoughDisk {
                needed: 1100,
                have: 1099
            })
        );
        let enough = SystemInfo {
            total_memory_bytes: 0,
            free_disk_bytes: 1100,
        };
        assert!(stage_availability(enough, &m).ok);
    }

    #[test]
    fn builtin_models_are_available_on_8gb_class_machine() {
        // OS が 8GB の PC で報告する総メモリ（約 7.6GiB）でも判定が通ること
        let info = SystemInfo {
            total_memory_bytes: 7 * GIB + GIB / 2,
            free_disk_bytes: 10 * GIB,
        };
        let catalog = crate::catalog::Catalog::builtin().unwrap();
        for m in &catalog.models {
            assert!(stage_availability(info, m).ok, "{}", m.id);
        }
    }
}
