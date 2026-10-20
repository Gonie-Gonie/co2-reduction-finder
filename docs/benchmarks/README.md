# 모델 비교 산출물

2026-10-20에 수행한 원본 Python/Keras H5 모델과 Rust 포팅 모듈의 실행 시간·배포 용량 비교입니다. 결과를 읽으려면 아래 보고서부터 확인하면 됩니다.

## 보고서와 보존한 결과

| 파일 | 용도 |
| --- | --- |
| [측정 보고서](../model-benchmark.md) | 시간·용량 비교표, 측정 조건, 출력 오차, 초기화 시간, 재실행 명령 |
| [2026-10-20.json](2026-10-20.json) | 해당 측정의 환경, 반복별 시간, 출력 오차, 패키지별 용량을 보존한 기준 데이터 |

1,000개 입력 기준으로 Rust는 소형·중간 모델에서 TensorFlow 그래프 실행보다 빨랐고, 가장 큰 모델은 TensorFlow 그래프 실행이 빨랐습니다. 일반 설치 파일 기준 Python 실행 환경과 H5 40개는 약 **1.54 GiB**, 모델·GUI를 포함한 Rust GNU release EXE는 **30.35 MiB**였습니다.

보고서의 `graph`는 **`tf.function` 기반 직접 모델 호출**을 뜻합니다. 일반 `predict()`도 내부적으로 그래프를 사용하므로, 두 열의 차이는 입력 사전 준비·배치 크기·데이터 처리 비용을 함께 줄인 효과입니다. 최소 배포 용량이나 순수 언어 속도의 비교로 해석하지 않습니다.

## 재실행 코드

| 파일 | 용도 |
| --- | --- |
| [scripts/benchmark_models.py](../../scripts/benchmark_models.py) | 공통 입력 생성, Python 세 가지 호출 방식과 Rust 순차 실행, 시간·출력 비교 |
| [examples/model_benchmark.rs](../../examples/model_benchmark.rs) | 앱의 Rust 모델 로더와 추론 경로를 직접 사용하는 벤치마크 실행 파일 |
| [scripts/measure_runtime_size.py](../../scripts/measure_runtime_size.py) | Python 필수 의존 패키지·실행기·모델 및 Rust EXE 파일 용량 집계 |

모델 원본은 `models/ann/v1/h5/`, 모델 구성과 분할 비율은 `models/ann/v1/model_registry.json`, Rust가 내장하는 변환 가중치는 `assets/models.c2m`입니다.

## 로컬 원시 결과

재실행 시 생성되는 파일은 **`target/benchmark/`**에 모입니다. 이 폴더는 Git에서 제외됩니다.

| 파일 | 내용 |
| --- | --- |
| `inputs.json` | 두 구현에 공통으로 사용한 입력 |
| `python.json` | Python 호출 방식별 반복 시간과 전체 예측 출력 |
| `rust.json` | Rust 반복 시간과 전체 예측 출력 |
| `summary.json` | 시간 비율·출력 오차·환경 정보를 합친 결과 |
| `runtime_size.json` | 의존성 버전, 파일 수, 용량과 실행 파일 경로 |
| `archive/runtime_size-initial-2026-10-20.json` | EXE 빌드 전의 초기 용량 결과. 최종 비교에는 사용하지 않는 보관본 |

여기의 파일은 다음 실행에서 갱신됩니다. 날짜가 붙은 위의 기준 JSON은 별도로 보존됩니다. 다른 측정은 `--output-dir`로 작업 폴더를 지정할 수 있습니다.

## 로컬 실행 파일과 빌드 보조 파일

| 경로 | 용도 |
| --- | --- |
| `target/x86_64-pc-windows-gnu/release/co2-reduction-finder.exe` | 이번에 용량을 측정한 전체 GUI 앱: 31,822,848 bytes |
| `target/x86_64-pc-windows-gnu/release/examples/model_benchmark.exe` | 이번에 시간을 측정한 Rust 추론 벤치마크: 23,796,224 bytes |
| `target/tooling/shlwapi.def`, `target/tooling/libshlwapi.a` | 이 PC의 GNU 빌드에 사용한 Windows 시스템 DLL용 import library 정의·생성물 |

실행 파일과 빌드 보조 파일도 Git에서 제외됩니다. GNU 빌드 보조 파일을 만드는 방법과 일반 Windows 빌드 명령은 [측정 보고서](../model-benchmark.md#reproduce)에 있습니다.
