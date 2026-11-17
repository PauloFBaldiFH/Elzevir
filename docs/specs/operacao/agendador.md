# Agendamento e histórico — A6/P10

O servidor mantém um trabalhador operacional por instituição, separado da UI.
Uma solicitação aceita recebe identificador antes do trabalho de disco e só há
um trabalho ativo. O histórico contém até 64 resultados sem mensagens privadas;
interrupções de processo são apresentadas como falha, nunca como sucesso.
Ao encerrar o servidor, o trabalhador termina o ponto seguro corrente antes de
liberar a trava e o banco. Fechar a UI não interrompe o trabalho.

`operations.toml` é um arquivo tipado, limitado a 64 KiB, escrito atomicamente.
O padrão não agenda nada. A política local define retenção de 1 a 365 cópias e
horário diário em fuso IANA explícito (UTC por padrão). O cálculo de vencimento
recebe relógio/fuso como entrada: uma execução por data civil, inclusive depois
de reinício; mudança de horário de verão não duplica a execução. Uma execução
perdida hoje ocorre ao iniciar após a hora; dias anteriores não são repetidos.
A UI lista política, trabalhos, artefatos e resultados do mesmo servidor.

A retenção considera apenas nomes de backup gerados pelo Elzevir, arquivos
regulares concluídos e sem links. Nunca remove arquivos desconhecidos ou a
cópia mais nova. A falha de limpeza não invalida o artefato já publicado.
