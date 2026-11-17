# Elzevir — gerador de mascotes e ícones em Rust

Este pacote reúne **quatro artes originais**, geradas por funções Rust:

- `src/assets/caranguejo_bibliotecario.rs` — caranguejo de óculos, paletó e livro; também versão recortada.
- `src/assets/caranguejo_bibliotecario_busca.rs` — caranguejo com lupa, expressão de dúvida e estante; ideal para tela 404 e buscas sem resultados.
- `src/assets/caranguejo_leitor.rs` — caranguejo com óculos lendo um livro; versões com e sem fundo.
- `src/assets/elzevir_tray_icon.rs` — o ícone minimalista de bandeja, com o casco, olhos e livro.

**Os desenhos são código vetorial, não PNG/JPG codificados em base64 ou arquivos de imagem externos.**

## Instalar e gerar

Requer Rust/Cargo e acesso às dependências na primeira compilação (`resvg`, `ico`).

```bash
cargo run --release
```

Saída: `generated-assets/`, com SVGs, PNGs (16, 24, 32, 48, 64, 128, 256, 512 px) e ICOs multi-resolução (16, 20, 24, 32, 40, 48, 64, 128, 256 px).

```bash
cargo run --release -- --only elzevir_tray_icon
cargo run --release -- --only caranguejo_bibliotecario_busca
cargo run --release -- --svg-only
cargo run --release -- --out ./assets-gerados
```

## Integrar ao repositório principal

Você pode copiar a pasta `src/assets` para um módulo da aplicação Elzevir e adaptar o comando de exportação ao seu fluxo. Se não quiser manter os binários gerados no Git, mantenha `generated-assets/` no `.gitignore`.

Os SVGs gerados podem ser consumidos diretamente por interfaces web; no Windows, o `.ico` final pode ser incorporado no executável como recurso através do processo de compilação/empacotamento do app.

**Aviso:** este ambiente não dispõe de `cargo`/`rustc`, portanto os arquivos Rust foram preparados sem validação por compilação local. As APIs das crates foram conferidas na documentação; compile e execute no ambiente de desenvolvimento.

## Licença

O código foi preparado para integração ao seu repositório. Verifique a compatibilidade de licenças das dependências antes de distribuir, conforme as políticas do seu projeto.
