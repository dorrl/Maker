# Maker Browser

Maker Browser는 HTML과 CSS를 Blitz로 그리고, 페이지에 기본 인라인 스크립트가 있을 때 QuickJS를 여는 Windows용 브라우저입니다.

```powershell
cargo run --release --manifest-path browser/Cargo.toml
cargo run --release --manifest-path browser/Cargo.toml -- .\page.html
cargo run --release --manifest-path browser/Cargo.toml -- https://example.com/
```

주소창에 URL이나 HTML 파일의 경로를 입력하거나 **파일 열기**를 누릅니다. 로컬 HTML은 같은 폴더와 하위 폴더의 상대 CSS 및 이미지 경로를 읽을 수 있습니다. **Ctrl+T**로 새 탭을 열고, **Ctrl+Tab**으로 전환하며, **Ctrl+W**로 닫습니다.

현재 탭의 페이지는 계속 유지합니다. 다른 탭은 마지막으로 열거나 선택한 시점부터 30초가 지나면 렌더링 문서, JavaScript 실행 상태, 이미지 및 진행 중인 로드를 해제합니다. 절전 탭을 선택하면 남겨 둔 주소에서 페이지를 다시 읽습니다. 주소와 방문 기록은 유지되며, 페이지 안에서 변경한 내용은 다시 로드할 때 초기화됩니다.

인라인 스크립트는 기본 DOM 조회, 텍스트 변경 및 클릭 이벤트를 지원합니다. 외부 또는 모듈 스크립트, 전체 브라우저 DOM API, 동영상과 복잡한 웹 앱은 현재 지원 범위 밖입니다.

메모리 사용량은 Windows 작업 관리자 **세부 정보**의 **메모리(전용 작업 집합)** 열에서 `maker-browser.exe`로 확인할 수 있습니다. 100MB는 가벼운 페이지를 표시하는 릴리스 빌드의 목표입니다.

Windows에서 960×680 크기의 릴리스 빌드로 확인한 전용 작업 집합은 빈 탭 약 1.5 MiB, 외부 CSS·1024×768 PNG·인라인 JS가 있는 로컬 페이지 약 7.9 MiB, 같은 구성의 HTTP 페이지와 로컬 탭을 함께 유지할 때 약 11.6 MiB였습니다. 빈 탭으로 전환하고 두 페이지가 절전 상태에 들어간 뒤에는 약 3.1 MiB였습니다.
