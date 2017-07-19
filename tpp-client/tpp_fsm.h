
/**
 * @file: tpp_fsm.h
 * @brief TPP FSM header file
 */

#ifndef TPP_FSM_H_
#define TPP_FSM_H_

// TPP FSM state definitions per ECE358 S17 P2 spec

#define	TPPS_CLOSED		0	/* closed */
#define	TPPS_LISTEN		1	/* listening for connection */
#define	TPPS_SYN_SENT		2	/* active, have sent syn */
#define	TPPS_SYN_RECEIVED	3	/* have send and received syn */
/* states < TPPS_ESTABLISHED are those where connections not established */
#define	TPPS_ESTABLISHED	4	/* established */

#define	TPPS_HAVERCVDSYN(s)	((s) >= TPPS_SYN_RECEIVED)


#endif // !TPP_FSM_H_
