# Marco B — central de backups

O percurso começa em **Central de backups → Criar backup**. A instituição mantém a cópia cifrada na pasta gerenciada. Escolher outra pasta solicita também uma exportação. Cada cópia tem identidade própria, que acompanha o operador ao abrir detalhes, verificar, exportar ou preparar SCP.

## Capturas do painel em execução

| Tela | O que conferir |
| --- | --- |
| [Central em 160×45](central-160x45.png) · [120×30](central-120x30.png) · [80×24](central-80x24.png) | Duas cópias reais. A seleção da mais antiga revela seu arquivo e o resultado da exportação. |
| [Backup manual em 160×45](backup-manual-160x45.png) · [80×24](backup-manual-80x24.png) | Navegação por pastas, caminho editável, criação de subpasta e seleção de arquivo. Em 80 colunas, **Locais** e **Formato e espaço** abrem os respectivos painéis. |
| [Conclusão](conclusao-160x45.png) · [Preparar SCP](transferir-scp.png) | Caminho, tamanho e verificação na criação. A preparação do comando usa a cópia selecionada e o destino SSH digitado pelo operador. |
| [Falha ao exportar](exportacao-falhou.png) · [Histórico](historico-160x45.png) | O destino já contém um arquivo de mesmo nome. A confirmação da cópia local e o erro de exportação aparecem separadamente. |
| [Propostas 02–04 ao lado do terminal](comparacao.png) | As propostas fornecidas aparecem à esquerda. À direita, cada imagem foi capturada do executável. |

Na comparação com as propostas, o explorador conserva as três regiões em tela larga. Em tela compacta dá prioridade à listagem. A conclusão mantém a recuperação da chave junto às opções de verificação e agrupa as ações de transferência por arquivo. No histórico, recibos antigos sem identidade do artefato aparecem como operações gerais. O horário diário vem da política da instituição. A hora efetiva da próxima execução e a duração do último backup não estão disponíveis no contrato consultado.

## Como o fluxo foi validado

O ensaio usa o binário Rust, uma instituição temporária, o servidor em loopback e um pseudoterminal Linux. Os cliques são pares de eventos de pressionar e soltar nas coordenadas das células. As capturas reproduzem as células recebidas do terminal.

1. Cancelar a criação deixa o histórico de trabalhos vazio.
2. Colar um caminho absoluto, criar uma subpasta e escolher **Criar backup** gera a cópia gerenciada e a cópia adicional. O ensaio compara os hashes SHA-256.
3. O painel exporta a chave para um arquivo privado. A CLI usa essa chave para autenticar a cópia exportada e verificar a integridade do banco.
4. Com os detalhes abertos, a CLI pede um segundo backup. O comando SCP mantém o nome do primeiro. Ao tentar exportá-lo novamente para o destino ocupado, o teste confere a falha e compara os bytes dos dois arquivos existentes.
5. **Verificar arquivo** inicia um trabalho com a identidade do primeiro backup. O teste espera o recibo de conclusão, seleciona a linha da cópia mais antiga e confirma a seleção após redimensionar a tela.
6. Com o servidor parado, o atalho **e** abre o explorador para exportar a cópia retida para outro destino. O teste compara seus bytes com os da cópia local.

O painel reutiliza o catálogo, a verificação nativa, os trabalhos do servidor, os recibos do host e a publicação de arquivos sem sobrescrita. O explorador consulta pastas em segundo plano. As dependências `fs4` e `jiff` já integravam a árvore do projeto e fornecem espaço disponível e data de modificação.

## Reprodução

Execute os comandos na raiz do repositório. Os ensaios do terminal usam Python, `pyte`, Pillow e a fonte DejaVu Sans Mono.

```sh
cargo build -p elzevir --locked
python3 tools/panel/capture_backups.py --output /tmp/elzevir-marco-b
python3 tools/panel/capture_dashboard.py --output /tmp/elzevir-marco-a
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo xtask ci
cargo nextest run --workspace --locked --run-ignored all --test-threads 4
cargo deny check
```

O ensaio simula a indicação de sessão SSH e verifica o comando preparado. A transferência para outro host, a abertura de pasta em um desktop gráfico, a navegação nativa no Windows e o envio autenticado a um provedor de nuvem dependem desses ambientes. A revisão de nuvem e agendamentos pertence ao Marco C.

## Resultado da execução

Os 1.217 testes da suíte passaram, incluindo os opcionais. Formatação, Clippy, `xtask ci` e as quatro verificações de `cargo deny` passaram. O Cargo registrou um aviso de compatibilidade futura de `proc-macro-error2`. O ensaio do dashboard do Marco A também passou.

O percurso do Marco B passou em duas execuções consecutivas e em uma terceira com o atalho de exportação. Durante a validação, recompilar o binário compartilhado enquanto o painel rodava interferia no lançamento do processo de exportação. Agora cada ensaio inicia a partir de uma cópia privada do executável. Uma execução preliminar da suíte também ficou presa no teste de fila cheia de `elzevir-store`. O teste passou isoladamente e na execução completa com quatro processos de teste.
