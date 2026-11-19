# Painel de administração no terminal — P1

## 1. Comportamento

Primeira entrega do painel previsto no doc 04, §7, baseada na análise
`painel-tui-analise.md` de 2026-11-16 fornecida por Paulo.

A [P2](painel-controle.md) acrescenta o modo conectado com `--config` ou
`--data-dir`, sem exigir `--demo`. O comportamento da demonstração permanece.

1. `elzevir panel --demo` abre uma TUI ratatui no mesmo binário, sem privilégios,
   banco, rede, comandos externos ou alterações no sistema.
2. O modo demonstração é explícito e obrigatório nesta etapa. `panel` sem
   `--demo` recusa a operação antes de modificar o terminal.
3. As seis seções são dados: painel de controle, diagnóstico, endereços e acesso,
   backup e agendamentos, atualizações e reiniciar. Todas mostram conteúdo
   demonstrativo, sem oferecer operações reais.
4. O trabalho aparece à esquerda e o contexto da instituição selecionada à
   direita. A instituição é fictícia e identificada como demonstração.
5. Tab/setas percorrem as seções, com retorno ao início; d/x/a/b/u/r selecionam
   diretamente; Ctrl+P abre a paleta; Enter/Esc fecham a paleta; q/Ctrl+C saem.
   Eventos de soltura de tecla são ignorados. Mouse seleciona uma seção no menu
   ou na paleta. Cliques fora desses elementos não mudam a seleção.
6. Textos vêm de Fluent, com `--lang en` (padrão) e `--lang pt-BR`.
   `--ascii` usa bordas ASCII; nenhuma informação depende somente de cor.
7. A animação avança por ticks de 150 ms; a leitura de eventos tem timeout.
   Não há tarefas externas nesta etapa. Terminais menores que 80×18 mostram
   orientação para redimensionar e continuam permitindo sair.
8. Entrada e saída precisam ser terminais interativos. O estado do terminal
   (modo raw, tela alternativa e captura do mouse) é restaurado ao sair e ao
   propagar erro. O hook do ratatui restaura o terminal em caso de panic.

## 2. Origem no Koha

Não porta comportamento do Koha. Inspiração de fluxos do painel Koha Nexus,
conforme a análise fornecida; não copia código Python ou Bash.

## 3. Paridade

Livre. ratatui 0.30 e seu backend crossterm (MIT) desenham e controlam o
terminal. Fluent e insta já são dependências do workspace. A nova crate
`elzevir-panel` não depende de armazenamento, web ou domínio.

## 4. Casos de borda

- Navegar antes da primeira e depois da última seção mantém seleção válida.
- Soltar uma tecla não repete navegação; Ctrl+C permite sair da paleta.
- Redimensionar e clicar fora do menu não causa panic nem dispara operações.
- Mensagens de ambas as línguas existem para todas as seções.
- Renderizações determinísticas com TestBackend são comparadas por snapshot.
- O comando sem `--demo` e sem TTY falha com mensagem útil.

## 5. Desvios

Nenhum: não há área de paridade nesta entrega.

## 6. Fora do escopo

Estado real do servidor, canal administrativo, backups, serviços, instalação,
releases, túnel, broker, rclone e reinício real. Dependem das etapas A/P
posteriores da análise. Não há alegação de validação em Windows nesta máquina.

## Navegação interativa

A apresentação interativa usa cores consistentes, barra superior com
versão, estado observado do SQLite e Tantivy e navegação por números.
As áreas clicáveis acompanham a geometria do quadro e o modal ativo.
Atalhos e cliques enviam as mesmas intenções ao controlador. Confirmações
continuam exigindo ação explícita. A demonstração permanece isolada.

A aba Acervo usa o canal administrativo privado do sistema operacional
e as portas do catálogo para busca, criação mínima e edição de título
com a versão exibida. Excluir aparece na lista de políticas, onde a
escritora já recusa políticas em uso. Os exemplares usam os sinais
atuais de disponibilidade. A consulta apresenta até dez instâncias ou
vinte políticas por seleção. A busca usa a listagem do Tantivy.
