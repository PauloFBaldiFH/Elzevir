# Diagnóstico e logs — P3

A [P4](enderecos-acesso.md) reutiliza e amplia a sonda HTTP para os três modos
da equipe, incluindo formulário de login, Host explícito sem DNS e IP específico
confirmado em interface local. Cada requisição passa a ter prazo de 500 ms.

## 1. Comportamento

1. A seção Diagnóstico (`x`) do painel conectado mostra serviço, saúde do banco
   e indexador, espaço disponível, volume local e acesso básico à pasta de dados.
   Continua usando o único worker da P2, sem I/O na thread de desenho.
2. `v` executa verificações adicionais nesta seção; `e` executa e exporta um ZIP
   novo na pasta de trabalho. A UI mostra andamento e resultado/caminho, sem
   esperar pelo worker para navegar ou sair. Pedidos repetidos são coalescidos;
   exportar inclui verificar e tem prioridade sobre atualização simples.
3. `elzevir diagnose --config ARQUIVO` (ou `--data-dir PASTA`) oferece a mesma
   operação fora da TUI, com saída JSON. `--output ARQUIVO.zip` exporta sem
   sobrescrever arquivos existentes; o diretório pai precisa existir. As
   verificações negativas são resultados do relatório, não falhas da CLI.
4. `PRAGMA quick_check(1)` é explícito, pelo IPC privado, usando o pool de
   leitura do servidor. O painel e a CLI não abrem SQLite. Uma única checagem
   pode executar por processo; o permit permanece com o trabalho mesmo após
   cancelamento do cliente. Um progress handler interrompe trabalho SQLite
   após aproximadamente um segundo de CPU/execução; I/O bloqueado pelo SO não
   é cancelável. Não há migração, checkpoint, reparo nem teste de escrita.
5. O comando IPC `diagnose` estende o schema 1 com resposta pequena, identidade,
   PID e resultado tipado. Preserva timeout de dois segundos, limite de 4 KiB
   e validação da A4. Servidor legado/ocupado/erro produz desconhecido, nunca OK.
6. O teste HTTP explícito consulta somente `/` do listener observado em loopback
   (wildcard é convertido para loopback), sem proxy/redirect e com timeout de
   um segundo. HTTP 200 é sucesso; outro resultado é falha. Bind específico
   fora de loopback fica não verificado até a P4. Não testa login nem internet.
7. Espaço disponível zero é falha; outras quantidades são medições, sem afirmar
   capacidade de backup. Volume local reutiliza o verificador da A2. Em Unix,
   acesso básico verifica leitura/escrita/travessia sem criar arquivos; no
   Windows, onde não há inspeção equivalente implementada, fica desconhecido.
8. O horário é exibido em UTC; sincronização do relógio não é comprovada.
   Túnel e release mais nova são explicitamente não verificados nesta entrega.
9. Verificações explícitas têm horário próprio e são invalidadas ao perder a
   identidade confirmada do servidor ou ao reiniciá-lo. `f` atualiza observações
   leves e logs, sem repetir `quick_check`. Observação antiga segue a P2.
10. Logs ao vivo são uma janela limitada: no máximo 64 entradas de diretório,
    sete arquivos `elzevir.AAAA-MM-DD.jsonl`, 64 KiB por coleta e 40 eventos.
    Links e diretórios inesperados são recusados. Leitura começa no final dos
    arquivos mais novos, ignora linhas parciais e sinaliza janela limitada.
11. Só entram eventos conhecidos de `elzevir::ops`, instituição `local`, com
    identidade válida, timestamp UTC válido e nível conhecido. UI/ZIP recebem
    apenas horário, nível e código do evento: mensagem, caminhos, queries,
    credenciais e campos adicionais nunca são copiados. Linhas inválidas ou
    desconhecidas são omitidas e contadas. Sem arquivo, erro e lista vazia
    são situações distintas. Não há busca por conteúdo arbitrário de log.
12. O ZIP contém somente `report.json` e `events.jsonl`, com schema e versão do
    cliente, estado/checks e medições permitidas. Não inclui TOML, banco,
    leitores, nomes da instituição, endereços, identidades ou logs brutos. É
    gerado em temporário oculto e publicado sem sobrescrita, com permissão privada
    em Unix. O ZIP não é enviado automaticamente a nenhum serviço.
13. Todos os textos da UI usam Fluent en/pt-BR. O modo demo continua independente
    de dados reais e sem exportação.

## 2. Origem no Koha

Não porta comportamento do Koha. Base: P3 da análise de painel fornecida por
Paulo e contratos de saúde/IPC das etapas A3/A4.

## 3. Paridade

Livre. ZIP sem compressão usa o formato ZIP32 documentado (dois arquivos
pequenos, CRC32, UTF-8), sem dependência adicional. `jiff` já usado no workspace
valida timestamps. A feature `hooks` do rusqlite permite limitar a checagem.

## 4. Casos de borda

- Checagem concorrente recusada, erro/servidor antigo sem falso positivo.
- Identidade da resposta conferida; saúde barata continua separada de integridade.
- Logs grandes, rotação, linha parcial, JSON inválido, links, eventos desconhecidos
  e campos com segredos não vazam para a tela nem para o ZIP.
- Arquivo de saída existente preservado; falha não deixa relatório parcial.
- Exportação não repete `quick_check` em cada atualização; sair não espera I/O.
- Tela, teclas, estados e exportação cobertos em en/pt-BR e terminal Linux real.

## 5. Desvios

Nenhum: não há área de paridade.

## 6. Limites

Não é auditoria completa, `integrity_check`, teste de restauração, prova de
sincronização NTP ou de ACLs do Windows. A janela de logs não é uma contagem
de erros por 24h. Túnel, releases e testes de rede ampliados vêm depois. Queda
abrupta pode deixar temporário oculto; o nome final só aparece após a publicação.
