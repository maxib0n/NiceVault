# Nota sospetta

<script>alert("questo non deve girare")</script>
<img src=x onerror=alert(1)>
<iframe src="https://evil.example/"></iframe>
<form action="https://evil.example/"><input name="p"></form>
<svg onload="alert(1)"><circle r="5"/></svg>
<style>body { display: none }</style>
<a href="javascript:alert(1)">ancora</a>

[link pericoloso](javascript:alert(1)) · [vbscript](vbscript:msgbox(1)) · [file](file:///etc/passwd)
[dati](data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==) · [credenziali](https://user:pw@evil.example/)
[mailto con allegato](mailto:a@b.it?attach=/etc/passwd) · <javascript:alert(1)>

![traccia](https://evil.example/pixel.png "pixel")
![dati](data:image/svg+xml;base64,PHN2ZyBvbmxvYWQ9YWxlcnQoMSk+)
![fuori dal volume](../../../../etc/passwd)

[riferimento]: javascript:alert(2)

Un [riferimento][riferimento] e un'intestazione con id: <h1 id="app">x</h1>

$\href{javascript:alert(1)}{clic}$ e $\htmlStyle{color:red}{rosso}$ e $\url{https://evil.example}$

```mermaid
flowchart LR
  A["<img src=x onerror=alert(1)>"] --> B
  click A "javascript:alert(1)"
  click B call alert(1)
```
